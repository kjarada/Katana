#include "convert.hpp"

#include <algorithm>
#include <array>
#include <cstdint>
#include <fstream>
#include <functional>
#include <map>
#include <sstream>
#include <string_view>
#include <utility>

#include "customisation.hpp"
#include "map_file.hpp"
#include "style_library.hpp"
#include "katana/core/text.hpp"
#include "katana/core/text_encoding.hpp"
#include "katana/entity/colour_names.hpp"

namespace katana::archive12d {

namespace {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;
using katana::entity::LineStyle;
using katana::entity::SurveyRule;

[[nodiscard]] std::string inQuotes(std::string_view text)
{
    return "\"" + std::string(text) + "\"";
}

// `text` without the first of `words` that begins it, and without the blanks
// that followed the word. A text that is the word and nothing more keeps it:
// there the word is the name, not something put in front of one.
[[nodiscard]] std::string withoutLeadingWord(std::string_view text,
                                             const std::vector<std::string>& words)
{
    for (const std::string& word : words) {
        if (text.size() <= word.size() || !text.starts_with(word) || text[word.size()] != ' ') {
            continue;
        }
        const std::size_t rest = text.find_first_not_of(' ', word.size());
        if (rest == std::string_view::npos) {
            continue;
        }
        return std::string(text.substr(rest));
    }
    return std::string(text);
}

// What a word is made of, so that "the word" is not found inside a longer
// one. Every byte of a character outside ASCII counts: a letter this cannot
// classify is still part of the word it stands in.
[[nodiscard]] bool isWordByte(char c)
{
    return static_cast<unsigned char>(c) >= 0x80 || (c >= '0' && c <= '9') ||
           (c >= 'A' && c <= 'Z') || (c >= 'a' && c <= 'z') || c == '_';
}

// `text` without `word` wherever it stands as a whole word. The blanks around
// it go with it, and one is left only where it had blanks on BOTH sides and
// text beyond them: "a W b" is "a b", "a W, b" is "a, b", "W b" is "b".
[[nodiscard]] std::string withoutWord(std::string text, std::string_view word)
{
    std::size_t from = 0;
    while (true) {
        const std::size_t at = text.find(word, from);
        if (at == std::string::npos) {
            return text;
        }
        const std::size_t end = at + word.size();
        if ((at > 0 && isWordByte(text[at - 1])) || (end < text.size() && isWordByte(text[end]))) {
            from = at + 1;
            continue;
        }
        std::size_t left = at;
        while (left > 0 && text[left - 1] == ' ') {
            --left;
        }
        std::size_t right = end;
        while (right < text.size() && text[right] == ' ') {
            ++right;
        }
        const bool between = left < at && right > end && left > 0 && right < text.size();
        text.replace(left, right - left, between ? " " : "");
        from = left;
    }
}

// A file's text, as core::decodeText reads its bytes.
//
// An encoding that was INFERRED is said in `warnings` when anything rests on
// it. A file that is neither UTF-8 nor marked as UTF-16 is read by a guess,
// and its characters outside ASCII - a diameter sign in a definition's name -
// are what that guess makes of them: a wrong one is a wrong character in the
// customisation and an error nowhere. Said once a file, however often the
// file is named on the command line.
[[nodiscard]] Result<std::string> textOf(const LegacyFile& file, std::vector<std::string>& warnings)
{
    auto decoded = katana::core::decodeText(file.bytes);
    if (!decoded) {
        return makeError(decoded.error().code,
                         "cannot read " + file.name + ": " + decoded.error().describe());
    }
    if (decoded->guessed) {
        // Each character outside ASCII is one lead byte of the UTF-8 it became.
        const auto outside =
            std::count_if(decoded->text.begin(), decoded->text.end(), [](char c) {
                return (static_cast<unsigned char>(c) & 0xC0U) == 0xC0U;
            });
        if (outside != 0) {
            std::string warning = file.name +
                                  ": neither UTF-8 nor marked as UTF-16, so read as " +
                                  katana::core::toString(decoded->encoding) +
                                  " by inference; characters outside ASCII that rest on it: " +
                                  std::to_string(outside);
            if (std::find(warnings.begin(), warnings.end(), warning) == warnings.end()) {
                warnings.push_back(std::move(warning));
            }
        }
    }
    return std::move(decoded->text);
}

// The run of `//` lines a file opens with, each without its marker and the
// one blank after it and otherwise as written. Blank lines above the run are
// passed over; the first line that is not a comment ends it.
[[nodiscard]] std::vector<std::string> leadingCommentBlock(std::string_view text)
{
    std::vector<std::string> lines;
    for (const std::string_view line : katana::core::splitLines(text)) {
        const std::size_t first = line.find_first_not_of(" \t");
        std::string_view rest = first == std::string_view::npos ? std::string_view{}
                                                                : line.substr(first);
        if (!rest.starts_with("//")) {
            if (rest.empty() && lines.empty()) {
                continue;
            }
            break;
        }
        rest.remove_prefix(2);
        if (rest.starts_with(' ')) {
            rest.remove_prefix(1);
        }
        lines.emplace_back(rest);
    }
    return lines;
}

// A plot pen, by its FOLDED name (entity::foldColourName): "pen", digits and
// at most one letter - "pen 035", "pen 7", "pen 12a". ConvertOptions::colours
// says why such a name is never given a colour.
[[nodiscard]] bool isPlotPen(std::string_view folded)
{
    if (!folded.starts_with("pen")) {
        return false;
    }
    std::size_t at = 3;
    while (at < folded.size() && folded[at] == ' ') {
        ++at;
    }
    const std::size_t digits = at;
    while (at < folded.size() && folded[at] >= '0' && folded[at] <= '9') {
        ++at;
    }
    if (at == digits) {
        return false;
    }
    if (at < folded.size() && folded[at] >= 'a' && folded[at] <= 'z') {
        ++at;
    }
    return at == folded.size();
}

struct TableColour {
    katana::entity::Color colour{};
    std::size_t line = 0;      // counted from 1
    std::size_t otherLine = 0; // a later line giving the name ANOTHER colour; 0 for none
};

// The colour table by folded name. A name given twice keeps its first colour
// and remembers the second line, to be refused only if a rule asks for that
// name: a table of nine hundred colours need not be sound where it is unused.
[[nodiscard]] Result<std::map<std::string, TableColour>>
readColourTable(const LegacyFile& table, std::vector<std::string>& warnings)
{
    const auto text = textOf(table, warnings);
    if (!text) {
        return text.error();
    }
    std::map<std::string, TableColour> colours;
    std::size_t number = 0;
    for (const std::string_view line : katana::core::splitLines(*text)) {
        ++number;
        std::string_view rest = katana::core::trimmed(line);
        if (rest.empty() || rest.starts_with("//")) {
            continue;
        }
        const auto refused = [&] {
            return makeError(ErrorCode::InvalidArgument,
                             table.name + ": line " + std::to_string(number) +
                                 " of the colour table is not `R G B <index> \"name\"`",
                             std::string(katana::core::trimmed(line)));
        };
        std::array<std::int64_t, 4> values{}; // red, green, blue, the table's own index
        for (std::int64_t& value : values) {
            const std::size_t end = rest.find_first_of(" \t");
            const auto parsed = katana::core::parseInteger(rest.substr(0, end));
            if (end == std::string_view::npos || !parsed) {
                return refused();
            }
            value = *parsed;
            rest = katana::core::trimmed(rest.substr(end));
        }
        const std::size_t close = rest.starts_with('"') ? rest.find('"', 1) : std::string_view::npos;
        if (close == std::string_view::npos || values[0] < 0 || values[0] > 255 || values[1] < 0 ||
            values[1] > 255 || values[2] < 0 || values[2] > 255) {
            return refused();
        }
        const katana::entity::Color colour{static_cast<std::uint8_t>(values[0]),
                                           static_cast<std::uint8_t>(values[1]),
                                           static_cast<std::uint8_t>(values[2]), 255};
        const auto [found, added] = colours.try_emplace(
            katana::entity::foldColourName(rest.substr(1, close - 1)), TableColour{colour, number, 0});
        if (!added && found->second.colour != colour && found->second.otherLine == 0) {
            found->second.otherLine = number;
        }
    }
    return colours;
}

// Gives the customisation a colour for every name its rules use that the
// standard names lack and the table has, and says which were left without -
// and which standard names the table would have coloured otherwise.
[[nodiscard]] Status resolveColours(const ConvertOptions& options,
                                    katana::entity::Customisation& customisation,
                                    ConvertReport& report)
{
    std::map<std::string, TableColour> table;
    if (options.colours) {
        auto read = readColourTable(*options.colours, report.warnings);
        if (!read) {
            return read.error();
        }
        table = std::move(*read);
    }
    // Each name once - by its fold, which is what a name is found by - as the
    // rules first write it.
    std::map<std::string, std::string> used;
    const auto use = [&](const std::string& name) {
        if (!name.empty()) {
            used.try_emplace(katana::entity::foldColourName(name), name);
        }
    };
    for (const SurveyRule& rule : customisation.map.rules()) {
        use(rule.colour);
        if (rule.symbol) {
            use(rule.symbol->colour);
        }
        if (rule.textStyle) {
            use(rule.textStyle->colour);
        }
    }
    for (const auto& [folded, name] : used) {
        if (const auto standard = katana::entity::standardColour(name)) {
            // The standard colour stands whatever the table says of the name:
            // a table may not redefine a standard name. Where the table does
            // say otherwise, that is a colour its author meant and this
            // cannot carry, so it is said rather than dropped.
            if (const auto other = table.find(folded);
                other != table.end() && other->second.colour != *standard) {
                report.coloursKeptStandard.push_back(
                    StandardColourKept{name, standard->toHex(), other->second.colour.toHex()});
            }
            continue;
        }
        const auto found = isPlotPen(folded) ? table.end() : table.find(folded);
        if (found == table.end()) {
            report.coloursUnresolved.push_back(name);
            continue;
        }
        if (found->second.otherLine != 0) {
            return makeError(ErrorCode::InvalidArgument,
                             options.colours->name + ": lines " +
                                 std::to_string(found->second.line) + " and " +
                                 std::to_string(found->second.otherLine) +
                                 " of the colour table give " + inQuotes(name) + " two colours");
        }
        if (auto status = customisation.colours.add(name, found->second.colour); !status) {
            return makeError(status.error().code,
                             "colour " + inQuotes(name) + ": " + status.error().describe());
        }
        report.coloursResolved.push_back(name);
    }
    return {};
}

// What the files gave, read in the order given.
struct Loaded {
    katana::entity::StyleLibrary library{};
    katana::entity::SurveyMap map{};
    // Every definition a later one of its name took the place of, as it was,
    // in the order they went.
    std::vector<LineStyle> dropped{};
    std::vector<std::string> warnings{};
};

// Reads one file into what is loaded. What a file IS is asked of its text
// (customisationKind) and never of its name, a later library wins a
// definition, and each survey code file's rules follow the last one's: what
// the loader of these files does (readCustomisationBytes). It is that loop
// over again because a conversion must also say what the loader does not
// keep - which definitions a file replaced, and whether its text was decoded
// by a guess.
[[nodiscard]] Status readInto(Loaded& loaded, const LegacyFile& file)
{
    const auto text = textOf(file, loaded.warnings);
    if (!text) {
        return text.error();
    }
    const std::string where = file.name + ": ";
    const auto kind = customisationKind(*text);
    if (!kind) {
        return makeError(kind.error().code, where + kind.error().describe());
    }
    std::vector<std::string> said;
    if (*kind == CustomisationFile::MapFile) {
        const std::size_t before = loaded.map.size();
        auto read = readMapFileInto(std::move(loaded.map), *text);
        if (!read) {
            return makeError(read.error().code, where + read.error().describe());
        }
        loaded.map = std::move(read->map);
        said = std::move(read->warnings);
        if (loaded.map.size() == before) {
            said.emplace_back("no rule was read from this survey code file");
        }
    } else {
        const std::size_t before = loaded.library.size();
        auto read = readStyleLibraryInto(std::move(loaded.library), *text, file.name);
        if (!read) {
            return makeError(read.error().code, where + read.error().describe());
        }
        loaded.library = std::move(read->library);
        said = std::move(read->warnings);
        // A file whose every definition replaced one still gave definitions.
        if (loaded.library.size() == before && read->replaced == 0) {
            said.emplace_back("no definition was read from this style library");
        }
        for (LineStyle& definition : read->replacedDefinitions) {
            loaded.dropped.push_back(std::move(definition));
        }
    }
    for (const std::string& warning : said) {
        loaded.warnings.push_back(where + warning);
    }
    return {};
}

// Whether two definitions of one name are one definition, leaving out what
// the FILE says of each rather than the block: where it came from, and which
// kind the file's name made it.
[[nodiscard]] bool sameDefinition(LineStyle a, LineStyle b)
{
    a.source.clear();
    b.source.clear();
    a.symbol = false;
    b.symbol = false;
    return a == b;
}

[[nodiscard]] std::string kindOf(bool symbol)
{
    return symbol ? "symbol" : "linestyle";
}

// Each definition that went, against the one the customisation holds, and
// which of the two the rules name it as. A definition kept as one kind where
// rules use the name as the OTHER kind is a warning besides: those rules are
// left naming a definition they were not written for, and the order of the
// libraries is all that decides it.
[[nodiscard]] Status reportReplaced(const Loaded& loaded, ConvertReport& report)
{
    for (const LineStyle& dropped : loaded.dropped) {
        const LineStyle* kept = loaded.library.find(dropped.name);
        if (kept == nullptr) {
            return makeError(ErrorCode::Internal, "definition " + inQuotes(dropped.name) +
                                                      " was replaced and is no longer defined");
        }
        ReplacedDefinition entry;
        entry.name = dropped.name;
        entry.keptFrom = kept->source;
        entry.droppedFrom = dropped.source;
        entry.keptAsSymbol = kept->symbol;
        entry.droppedAsSymbol = dropped.symbol;
        entry.differs = !sameDefinition(*kept, dropped);
        for (const SurveyRule& rule : loaded.map.rules()) {
            entry.linestyleRules += rule.linestyle == entry.name ? 1 : 0;
            entry.symbolRules += rule.symbol && rule.symbol->style == entry.name ? 1 : 0;
        }
        const std::size_t astray = entry.keptAsSymbol ? entry.linestyleRules : entry.symbolRules;
        if (entry.keptAsSymbol != entry.droppedAsSymbol && astray != 0) {
            report.warnings.push_back(
                entry.keptFrom + ": " + inQuotes(entry.name) + " is kept as a " +
                kindOf(entry.keptAsSymbol) + " in place of the " + kindOf(entry.droppedAsSymbol) +
                " of " + entry.droppedFrom + "; rules naming it as their " +
                kindOf(entry.droppedAsSymbol) + ": " + std::to_string(astray) +
                " (the library given last is the one kept)");
        }
        report.replaced.push_back(std::move(entry));
    }
    return {};
}

[[nodiscard]] std::string utf8(const std::filesystem::path& path)
{
    // Never path::string(), which gives a name outside ASCII in whatever the
    // Windows code page makes of it.
    const std::u8string text = path.u8string();
    return {reinterpret_cast<const char*>(text.data()), text.size()};
}

[[nodiscard]] Result<LegacyFile> readFile(const std::filesystem::path& path)
{
    std::ifstream file(path, std::ios::binary);
    if (!file) {
        return makeError(ErrorCode::NotFound, "cannot open this file", utf8(path));
    }
    std::ostringstream buffer;
    buffer << file.rdbuf();
    return LegacyFile{utf8(path.filename()), std::move(buffer).str()};
}

} // namespace

katana::core::Result<Conversion> convertLegacyCustomisation(const std::vector<LegacyFile>& files,
                                                            const ConvertOptions& options)
{
    if (options.name.empty()) {
        return makeError(ErrorCode::InvalidArgument,
                         "the customisation has no name: give one with --name <name>");
    }
    if (auto status = katana::entity::validateCustomisationName(options.name); !status) {
        return status.error();
    }
    if (files.empty()) {
        return makeError(ErrorCode::InvalidArgument,
                         "there is no file to convert: name the style libraries and survey code "
                         "files, in load order");
    }
    for (const std::string& word : options.stripLeadingWords) {
        if (word.empty()) {
            return makeError(ErrorCode::InvalidArgument,
                             "a word to take off the front of names (--strip-leading-word) is empty");
        }
    }
    for (const std::string& word : options.removeWords) {
        if (word.empty()) {
            return makeError(ErrorCode::InvalidArgument,
                             "a word to take out of comments (--remove-word) is empty");
        }
    }

    Loaded loaded;
    for (const LegacyFile& file : files) {
        if (auto status = readInto(loaded, file); !status) {
            return status.error();
        }
    }

    Conversion conversion;
    katana::entity::Customisation& customisation = conversion.customisation;
    ConvertReport& report = conversion.report;
    customisation.name = options.name;
    customisation.description = options.description;
    report.name = options.name;
    report.files = files.size();
    report.warnings = std::move(loaded.warnings);
    if (auto status = reportReplaced(loaded, report); !status) {
        return status.error();
    }

    // The notice. A file asked for one and holding none is refused: carrying
    // the author's notice with the data is the whole point of asking.
    std::vector<std::string> previous;
    for (const LegacyFile& file : options.noticeFrom) {
        const auto text = textOf(file, report.warnings);
        if (!text) {
            return text.error();
        }
        std::vector<std::string> block = leadingCommentBlock(*text);
        if (block.empty()) {
            return makeError(ErrorCode::InvalidArgument,
                             file.name + ": there is no // comment block at the head of this "
                                         "file to take a notice from");
        }
        if (block != previous) {
            customisation.notice.insert(customisation.notice.end(), block.begin(), block.end());
        }
        previous = std::move(block);
    }
    report.noticeLines = customisation.notice.size();

    // The definitions, each under its name without a leading word. `readAs`
    // is the name now -> the name in the files, which is what a second
    // definition arriving at one name is told by.
    const std::vector<std::string>& leading = options.stripLeadingWords;
    std::map<std::string, std::string, std::less<>> readAs;
    for (LineStyle definition : loaded.library.all()) { // in name order
        const std::string read = definition.name;
        definition.name = withoutLeadingWord(read, leading);
        std::string group = withoutLeadingWord(definition.group, leading);
        report.namesRenamed += definition.name != read ? 1 : 0;
        report.groupsRenamed += group != definition.group ? 1 : 0;
        definition.group = std::move(group);
        definition.source = options.name;
        const auto [first, added] = readAs.try_emplace(definition.name, read);
        if (!added) {
            return makeError(ErrorCode::InvalidArgument,
                             "taking the leading word off would give two definitions one name: " +
                                 inQuotes(first->second) + " and " + inQuotes(read) +
                                 " would both be " + inQuotes(definition.name));
        }
        report.symbols += definition.symbol ? 1 : 0;
        report.atVertices += definition.atVertices ? 1 : 0;
        report.strokes += definition.strokes.size();
        if (auto status = customisation.library.add(std::move(definition)); !status) {
            return makeError(status.error().code,
                             "definition " + inQuotes(read) + ": " + status.error().describe());
        }
    }
    report.definitions = customisation.library.size();
    report.linestyles = report.definitions - report.symbols;
    report.groups = katana::entity::styleGroups(customisation.library).size();

    // The rules, in the order they were read.
    std::size_t index = 0;
    for (SurveyRule rule : loaded.map.rules()) {
        const std::string entry = "codes[" + std::to_string(index) + "] " + inQuotes(rule.key);
        ++index;
        // "So that a rule still names the definition it named": a name that
        // WAS a definition's is renamed exactly as the definition was, so it
        // still is. What must not happen is a name no definition had coming to
        // be one - "W Kerb", defined nowhere, read as "Kerb", which is another
        // definition; or "Kerb", defined nowhere, met by "W Kerb" renamed.
        const auto reference = [&](std::string& name) -> Status {
            if (name.empty()) {
                return {};
            }
            std::string now = withoutLeadingWord(name, leading);
            if (!loaded.library.contains(name)) {
                if (const auto other = readAs.find(now); other != readAs.end()) {
                    return makeError(ErrorCode::InvalidArgument,
                                     entry + ": " + inQuotes(name) +
                                         " is the name of no definition, and taking the leading "
                                         "word off would make it name " +
                                         inQuotes(other->second));
                }
            }
            report.referencesRenamed += now != name ? 1 : 0;
            name = std::move(now);
            return {};
        };
        if (auto status = reference(rule.linestyle); !status) {
            return status.error();
        }
        if (rule.symbol) {
            if (auto status = reference(rule.symbol->style); !status) {
                return status.error();
            }
        }
        std::string group = withoutLeadingWord(rule.group, leading);
        report.ruleGroupsRenamed += group != rule.group ? 1 : 0;
        rule.group = std::move(group);
        std::string comment = rule.comment;
        for (const std::string& word : options.removeWords) {
            comment = withoutWord(std::move(comment), word);
        }
        report.commentsChanged += comment != rule.comment ? 1 : 0;
        rule.comment = std::move(comment);
        if (auto status = customisation.map.add(std::move(rule)); !status) {
            return makeError(status.error().code, entry + ": " + status.error().describe());
        }
    }
    report.rules = customisation.map.size();
    report.keys = customisation.map.keys().size();
    for (const std::string& name : customisation.map.stylesReferenced()) { // in name order
        if (!customisation.library.contains(name)) {
            report.unresolvedReferences.push_back(name);
        }
    }

    if (auto status = resolveColours(options, customisation, report); !status) {
        return status.error();
    }

    auto json = katana::entity::customisationToJson(customisation);
    if (!json) {
        return json.error();
    }
    // The file is the product of this tool, so it is read back before it is
    // handed over: a customisation that did not come back as itself would be
    // compiled into a build and found out there.
    const auto back = katana::entity::customisationFromJson(*json);
    if (!back || !(*back == customisation)) {
        return makeError(ErrorCode::Internal,
                         "the customisation written does not read back as itself",
                         back ? std::string{} : back.error().describe());
    }
    conversion.json = std::move(*json);
    report.bytes = conversion.json.size();
    return conversion;
}

katana::core::Result<Conversion> convertLegacyFiles(const ConvertRequest& request)
{
    // Every file is read before anything is converted or written, so a path
    // mistyped anywhere on the command line costs nothing.
    std::vector<LegacyFile> files;
    for (const std::filesystem::path& path : request.files) {
        auto file = readFile(path);
        if (!file) {
            return file.error();
        }
        files.push_back(std::move(*file));
    }
    ConvertOptions options;
    options.name = request.name;
    options.description = request.description;
    options.stripLeadingWords = request.stripLeadingWords;
    options.removeWords = request.removeWords;
    for (const std::filesystem::path& path : request.noticeFrom) {
        auto file = readFile(path);
        if (!file) {
            return file.error();
        }
        options.noticeFrom.push_back(std::move(*file));
    }
    if (!request.colours.empty()) {
        auto file = readFile(request.colours);
        if (!file) {
            return file.error();
        }
        options.colours = std::move(*file);
    }

    auto conversion = convertLegacyCustomisation(files, options);
    if (!conversion || request.output.empty()) {
        return conversion;
    }
    if (request.output.has_parent_path()) {
        std::error_code ignored; // a directory that cannot be made fails the open below
        std::filesystem::create_directories(request.output.parent_path(), ignored);
    }
    // Beside the target first, then over it: a write that fails part way
    // leaves the previous file, not half of a new one - and the previous file
    // is the customisation a build compiles in (the same idiom as the DXF
    // and IFC writers).
    std::filesystem::path partial = request.output;
    partial += ".partial";
    {
        // Binary, so that the line feeds the text ends its lines with are the
        // bytes written on every platform.
        std::ofstream out(partial, std::ios::binary | std::ios::trunc);
        out.write(conversion->json.data(), static_cast<std::streamsize>(conversion->json.size()));
        out.close();
        if (!out) {
            std::error_code ignored;
            std::filesystem::remove(partial, ignored);
            return makeError(ErrorCode::FileExportFailure, "cannot write this file",
                             utf8(request.output));
        }
    }
    std::error_code error;
    std::filesystem::rename(partial, request.output, error);
    if (error) {
        std::error_code ignored;
        std::filesystem::remove(partial, ignored);
        return makeError(ErrorCode::FileExportFailure,
                         "cannot put the file written in this one's place: " + error.message(),
                         utf8(request.output));
    }
    conversion->report.output = utf8(request.output);
    return conversion;
}

std::string toText(const ConvertReport& report)
{
    std::string text;
    const auto count = [&](std::string_view key, std::size_t value) {
        text += key;
        text += '=';
        text += std::to_string(value);
        text += '\n';
    };
    const auto quoted = [&](std::string_view key, std::string_view value) {
        text += key;
        text += '=';
        text += katana::core::replyQuoted(value);
        text += '\n';
    };
    const auto list = [&](std::string_view many, std::string_view one,
                          const std::vector<std::string>& values) {
        count(many, values.size());
        for (const std::string& value : values) {
            quoted(one, value);
        }
    };
    quoted("name", report.name);
    count("files", report.files);
    count("definitions", report.definitions);
    count("symbols", report.symbols);
    count("linestyles", report.linestyles);
    count("at_vertices", report.atVertices);
    count("groups", report.groups);
    count("strokes", report.strokes);
    count("definitions_replaced", report.replaced.size());
    for (const ReplacedDefinition& entry : report.replaced) {
        text += "definition_replaced=" + katana::core::replyQuoted(entry.name) +
                " kept=" + katana::core::replyQuoted(entry.keptFrom) +
                " kept_as=" + kindOf(entry.keptAsSymbol) +
                " dropped=" + katana::core::replyQuoted(entry.droppedFrom) +
                " dropped_as=" + kindOf(entry.droppedAsSymbol) +
                " differs=" + (entry.differs ? "yes" : "no") +
                " linestyle_rules=" + std::to_string(entry.linestyleRules) +
                " symbol_rules=" + std::to_string(entry.symbolRules) + "\n";
    }
    count("rules", report.rules);
    count("keys", report.keys);
    list("colours_resolved", "colour_resolved", report.coloursResolved);
    list("colours_unresolved", "colour_unresolved", report.coloursUnresolved);
    count("colours_kept_standard", report.coloursKeptStandard.size());
    for (const StandardColourKept& entry : report.coloursKeptStandard) {
        text += "colour_kept_standard=" + katana::core::replyQuoted(entry.name) +
                " standard=" + katana::core::replyQuoted(entry.standard) +
                " table=" + katana::core::replyQuoted(entry.table) + "\n";
    }
    count("names_renamed", report.namesRenamed);
    count("groups_renamed", report.groupsRenamed);
    count("references_renamed", report.referencesRenamed);
    count("rule_groups_renamed", report.ruleGroupsRenamed);
    count("comments_changed", report.commentsChanged);
    list("unresolved_references", "unresolved_reference", report.unresolvedReferences);
    count("notice_lines", report.noticeLines);
    list("warnings", "warning", report.warnings);
    count("bytes", report.bytes);
    if (!report.output.empty()) {
        quoted("output", report.output);
    }
    return text;
}

} // namespace katana::archive12d
