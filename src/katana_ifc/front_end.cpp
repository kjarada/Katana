#include "katana/ifc/front_end.hpp"

#include <algorithm>
#include <format>
#include <fstream>
#include <iterator>

#include "katana/core/path_text.hpp"
#include "katana/core/text.hpp"
#include "katana/survey/subsurface/utility_csv.hpp"

namespace katana::ifc {

using core::ErrorCode;
using core::makeError;
using core::Result;

namespace {

namespace sub = katana::survey::subsurface;

// Blank-separated words, a double-quoted word kept whole with its blanks.
// nullopt for a quote that is never closed.
std::optional<std::vector<std::string>> words(std::string_view text)
{
    std::vector<std::string> out;
    std::size_t at = 0;
    while (at < text.size()) {
        if (text[at] == ' ' || text[at] == '\t') {
            ++at;
            continue;
        }
        if (text[at] == '"') {
            const std::size_t end = text.find('"', at + 1);
            if (end == std::string_view::npos) {
                return std::nullopt;
            }
            out.emplace_back(text.substr(at + 1, end - at - 1));
            at = end + 1;
            continue;
        }
        const std::size_t end = text.find_first_of(" \t", at);
        out.emplace_back(text.substr(at, end == std::string_view::npos ? end : end - at));
        at = end == std::string_view::npos ? text.size() : end;
    }
    return out;
}

// The path, and what follows it: a quoted path, or everything up to the
// first ".ifc" that ends a word. nullopt when there is no .ifc; an error for
// a quote opened before a .ifc and never closed, which would otherwise pass
// to the other formats' grammar and be written under a name starting with
// the quote.
std::optional<Result<std::pair<std::string, std::string_view>>> splitPath(std::string_view argument)
{
    using Split = Result<std::pair<std::string, std::string_view>>;
    argument = core::trimmed(argument);
    if (argument.starts_with('"')) {
        const std::size_t end = argument.find('"', 1);
        if (end == std::string_view::npos) {
            const std::string lower = core::lowered(argument);
            for (std::size_t at = lower.find(".ifc"); at != std::string::npos;
                 at = lower.find(".ifc", at + 1)) {
                const std::size_t after = at + 4;
                if (after == lower.size() || lower[after] == ' ' || lower[after] == '\t') {
                    return Split(
                        makeError(ErrorCode::InvalidArgument, "a quoted path is never closed"));
                }
            }
            return std::nullopt;
        }
        std::string path(argument.substr(1, end - 1));
        if (!isIfcPath(pathFromUtf8(path))) {
            return std::nullopt;
        }
        return Split(std::pair(std::move(path), argument.substr(end + 1)));
    }
    const std::string lower = core::lowered(argument);
    for (std::size_t at = lower.find(".ifc"); at != std::string::npos;
         at = lower.find(".ifc", at + 1)) {
        const std::size_t end = at + 4;
        if (end == lower.size() || lower[end] == ' ' || lower[end] == '\t') {
            return Split(std::pair(std::string(argument.substr(0, end)), argument.substr(end)));
        }
    }
    return std::nullopt;
}

std::string upper(std::string_view text)
{
    std::string out(text);
    for (char& c : out) {
        c = static_cast<char>(std::toupper(static_cast<unsigned char>(c)));
    }
    return out;
}

core::Error invalid(std::string message)
{
    return makeError(ErrorCode::InvalidArgument, std::move(message));
}

// The file's whole text; nullopt when it cannot be read.
std::optional<std::string> readFile(const std::filesystem::path& path)
{
    std::ifstream file(path, std::ios::binary);
    if (!file) {
        return std::nullopt;
    }
    return std::string(std::istreambuf_iterator<char>(file), std::istreambuf_iterator<char>());
}

// `error`, with the file it is in named, as readIfcFile names its own.
core::Error inFile(core::Error error, const std::string& path)
{
    error.context = error.context.empty() ? path : path + ": " + error.context;
    return error;
}

} // namespace

std::filesystem::path pathFromUtf8(std::string_view text)
{
    // Core's, and not a conversion of this module's own: the one it had threw
    // on a name that is not UTF-8, which core's reads as the narrow name it is.
    return core::pathFromUtf8(text);
}

namespace {

constexpr std::string_view kExportOptions =
    "UTILITIES <schedule.csv>, SCHEMA <schema.csv>, RULES <rules.csv>, SPACING <m>, NODRAWING, "
    "NOENTITIES, SELECTED, NOALIGNMENTS, NOSURFACES, PREVIEW";
constexpr std::string_view kImportOptions =
    "LOCAL, NOALIGNMENTS, NOELEMENTS, NOSURFACES, TOLERANCE <m>, TAKECRS, KEEPCRS";

// A positive number of metres, or why not.
Result<double> metres(std::string_view word, std::string_view option)
{
    const auto value = core::parseFiniteDouble(word);
    if (!value || *value <= 0.0) {
        return invalid(std::string(option) + " takes a positive number of metres");
    }
    return *value;
}

// The words after the path, and the keywords met so far: a keyword given
// twice is refused, since the second would silently undo or repeat the first.
struct Keywords {
    std::vector<std::string> words;
    std::vector<std::string> seen;

    // True when `keyword` was met before; otherwise it is remembered.
    [[nodiscard]] bool again(const std::string& keyword)
    {
        if (std::find(seen.begin(), seen.end(), keyword) != seen.end()) {
            return true;
        }
        seen.push_back(keyword);
        return false;
    }
};

} // namespace

std::optional<Result<ExportArguments>> parseExportArguments(std::string_view argument)
{
    using Parsed = Result<ExportArguments>;
    const auto split = splitPath(argument);
    if (!split) {
        return std::nullopt;
    }
    if (!*split) {
        return Parsed(split->error());
    }
    ExportArguments out;
    out.path = (*split)->first;
    const auto found = words((*split)->second);
    if (!found) {
        return Parsed(invalid("a quoted path is never closed"));
    }
    Keywords keywords{*found, {}};
    bool noDrawing = false;
    for (std::size_t i = 0; i < keywords.words.size(); ++i) {
        const std::string keyword = upper(keywords.words[i]);
        const bool hasValue = i + 1 < keywords.words.size();
        const bool known = keyword == "UTILITIES" || keyword == "SCHEMA" || keyword == "RULES" ||
                           keyword == "SPACING" || keyword == "NODRAWING" ||
                           keyword == "NOENTITIES" || keyword == "SELECTED" ||
                           keyword == "NOALIGNMENTS" || keyword == "NOSURFACES" ||
                           keyword == "PREVIEW";
        const bool takesValue = keyword == "UTILITIES" || keyword == "SCHEMA" ||
                                keyword == "RULES" || keyword == "SPACING";
        if (!known || (takesValue && !hasValue)) {
            return Parsed(invalid(
                "\"" + keywords.words[i] +
                "\" is not an option of EXPORT <file.ifc>: " + std::string(kExportOptions)));
        }
        if (keywords.again(keyword)) {
            return Parsed(invalid(keyword + " is given twice"));
        }
        if (keyword == "UTILITIES") {
            out.schedule = keywords.words[++i];
        } else if (keyword == "SCHEMA") {
            out.schema = keywords.words[++i];
        } else if (keyword == "RULES") {
            out.rules = keywords.words[++i];
        } else if (keyword == "SPACING") {
            auto spacing = metres(keywords.words[++i], "SPACING");
            if (!spacing) {
                return Parsed(spacing.error());
            }
            out.spacing = *spacing;
        } else if (keyword == "NODRAWING") {
            noDrawing = true;
            out.entities = out.alignments = out.surfaces = false;
        } else if (keyword == "NOENTITIES") {
            out.entities = false;
        } else if (keyword == "SELECTED") {
            out.selected = true;
        } else if (keyword == "NOALIGNMENTS") {
            out.alignments = false;
        } else if (keyword == "NOSURFACES") {
            out.surfaces = false;
        } else {
            out.preview = true;
        }
    }
    if (out.schema && !out.schedule) {
        return Parsed(invalid("SCHEMA describes a schedule: give it with UTILITIES"));
    }
    if (out.selected && !out.entities) {
        return Parsed(invalid(std::string("SELECTED chooses among the entities, and ") +
                              (noDrawing ? "NODRAWING" : "NOENTITIES") + " leaves them out"));
    }
    return Parsed(std::move(out));
}

std::optional<Result<ImportArguments>> parseImportArguments(std::string_view argument)
{
    using Parsed = Result<ImportArguments>;
    const auto split = splitPath(argument);
    if (!split) {
        return std::nullopt;
    }
    if (!*split) {
        return Parsed(split->error());
    }
    ImportArguments out;
    out.path = (*split)->first;
    const auto found = words((*split)->second);
    if (!found) {
        return Parsed(invalid("a quoted path is never closed"));
    }
    Keywords keywords{*found, {}};
    for (std::size_t i = 0; i < keywords.words.size(); ++i) {
        const std::string keyword = upper(keywords.words[i]);
        const bool known = keyword == "LOCAL" || keyword == "NOALIGNMENTS" ||
                           keyword == "NOELEMENTS" || keyword == "NOSURFACES" ||
                           keyword == "TOLERANCE" || keyword == "TAKECRS" || keyword == "KEEPCRS";
        if (!known || (keyword == "TOLERANCE" && i + 1 == keywords.words.size())) {
            return Parsed(invalid(
                "\"" + keywords.words[i] +
                "\" is not an option of IMPORT <file.ifc>: " + std::string(kImportOptions)));
        }
        if (keywords.again(keyword)) {
            return Parsed(invalid(keyword + " is given twice"));
        }
        if (keyword == "LOCAL") {
            out.local = true;
        } else if (keyword == "NOALIGNMENTS") {
            out.alignments = false;
        } else if (keyword == "NOELEMENTS") {
            out.elements = false;
        } else if (keyword == "NOSURFACES") {
            out.surfaces = false;
        } else if (keyword == "TOLERANCE") {
            auto tolerance = metres(keywords.words[++i], "TOLERANCE");
            if (!tolerance) {
                return Parsed(tolerance.error());
            }
            out.tolerance = *tolerance;
        } else {
            if (out.takeCoordinateSystem) {
                return Parsed(invalid("TAKECRS and KEEPCRS say opposite things: give one"));
            }
            out.takeCoordinateSystem = keyword == "TAKECRS";
        }
    }
    if (!out.alignments && !out.elements && !out.surfaces) {
        return Parsed(
            invalid("NOALIGNMENTS, NOELEMENTS and NOSURFACES together leave nothing to import"));
    }
    if (out.local && out.takeCoordinateSystem.value_or(false)) {
        return Parsed(invalid("LOCAL moves the data out of the file's coordinate system, which "
                              "TAKECRS would give the project"));
    }
    return Parsed(std::move(out));
}

namespace {

// A path or a word as the grammar reads it back: always in double quotes,
// so that blanks, a ".ifc " inside a folder's name and a keyword-like name
// all read as the path. A double quote cannot be said inside one.
Result<std::string> quotedWord(std::string_view text)
{
    if (text.find('"') != std::string_view::npos) {
        return invalid("\"" + std::string(text) +
                       "\" holds a double quote, which a command line cannot quote");
    }
    return "\"" + std::string(text) + "\"";
}

} // namespace

Result<std::string> formatExportLine(const ExportArguments& arguments)
{
    auto path = quotedWord(arguments.path);
    if (!path) {
        return path.error();
    }
    std::string line = "EXPORT " + *path;
    for (const auto& [keyword, value] :
         {std::pair{"UTILITIES", &arguments.schedule}, std::pair{"SCHEMA", &arguments.schema},
          std::pair{"RULES", &arguments.rules}}) {
        if (*value) {
            auto quoted = quotedWord(**value);
            if (!quoted) {
                return quoted.error();
            }
            line += std::string(" ") + keyword + " " + *quoted;
        }
    }
    if (arguments.spacing) {
        line += " SPACING " + core::formatExactReal(*arguments.spacing);
    }
    if (!arguments.entities && !arguments.alignments && !arguments.surfaces) {
        line += " NODRAWING";
    } else {
        line += arguments.entities ? "" : " NOENTITIES";
        line += arguments.alignments ? "" : " NOALIGNMENTS";
        line += arguments.surfaces ? "" : " NOSURFACES";
    }
    line += arguments.selected ? " SELECTED" : "";
    line += arguments.preview ? " PREVIEW" : "";
    return line;
}

Result<std::string> formatImportLine(const ImportArguments& arguments)
{
    auto path = quotedWord(arguments.path);
    if (!path) {
        return path.error();
    }
    std::string line = "IMPORT " + *path;
    line += arguments.local ? " LOCAL" : "";
    line += arguments.alignments ? "" : " NOALIGNMENTS";
    line += arguments.elements ? "" : " NOELEMENTS";
    line += arguments.surfaces ? "" : " NOSURFACES";
    if (arguments.tolerance) {
        line += " TOLERANCE " + core::formatExactReal(*arguments.tolerance);
    }
    if (arguments.takeCoordinateSystem) {
        line += *arguments.takeCoordinateSystem ? " TAKECRS" : " KEEPCRS";
    }
    return line;
}

Result<ExportFiles> readExportFiles(const ExportArguments& arguments)
{
    if (arguments.schema && !arguments.schedule) {
        return invalid("SCHEMA describes a schedule: give it with UTILITIES");
    }
    ExportFiles out;
    if (arguments.schedule) {
        const std::filesystem::path path = pathFromUtf8(*arguments.schedule);
        const auto text = readFile(path);
        if (!text) {
            return makeError(ErrorCode::NotFound, "cannot read " + *arguments.schedule);
        }
        auto lines = sub::parseUtilityCsv(*text);
        if (!lines) {
            return inFile(lines.error(), *arguments.schedule);
        }
        UtilityInput utilities;
        utilities.lines = std::move(*lines);
        utilities.sourceName = path.filename().string();
        if (arguments.spacing) {
            utilities.grading.maximumDetectedSpacing = *arguments.spacing;
        }
        if (arguments.schema) {
            const auto schemaText = readFile(pathFromUtf8(*arguments.schema));
            if (!schemaText) {
                return makeError(ErrorCode::NotFound, "cannot read " + *arguments.schema);
            }
            auto parsed = sub::parseDeliverySchema(*schemaText);
            if (!parsed) {
                return inFile(parsed.error(), *arguments.schema);
            }
            out.schema = std::make_unique<sub::DeliverySchema>(std::move(*parsed));
            utilities.schema = out.schema.get();
        }
        out.utilities = std::move(utilities);
    }
    if (arguments.rules) {
        const auto text = readFile(pathFromUtf8(*arguments.rules));
        if (!text) {
            return makeError(ErrorCode::NotFound, "cannot read " + *arguments.rules);
        }
        auto rules = parseClassificationRules(*text);
        if (!rules) {
            return inFile(rules.error(), *arguments.rules);
        }
        out.rules = std::move(*rules);
        const auto& defaults = defaultClassificationRules();
        out.rules.insert(out.rules.end(), defaults.begin(), defaults.end());
    }
    return out;
}

Result<std::string> formatInfoLine(std::string_view path)
{
    auto quoted = quotedWord(path);
    if (!quoted) {
        return quoted.error();
    }
    return "INFO " + *quoted;
}

std::optional<Result<std::string>> parseRulesArguments(std::string_view argument)
{
    using Parsed = Result<std::string>;
    const auto found = words(argument);
    if (!found) {
        return Parsed(invalid("a quoted path is never closed"));
    }
    if (found->empty() || upper(found->front()) != "RULES") {
        return std::nullopt;
    }
    if (found->size() != 2) {
        return Parsed(invalid("usage: IFC RULES <file.csv>"));
    }
    return Parsed((*found)[1]);
}

Result<std::string> formatRulesLine(std::string_view path)
{
    auto quoted = quotedWord(path);
    if (!quoted) {
        return quoted.error();
    }
    return "IFC RULES " + *quoted;
}

Result<std::string> writeDefaultRules(std::string_view path)
{
    const std::filesystem::path file = pathFromUtf8(path);
    const auto& rules = defaultClassificationRules();
    std::ofstream out(file, std::ios::binary | std::ios::trunc);
    out << formatClassificationRules(rules);
    out.close();
    if (!out) {
        return makeError(ErrorCode::FileExportFailure, "the rules could not be written",
                         std::string(path));
    }
    return std::format("ifc rules file={} rules={}", core::replyQuoted(file.filename().string()),
                       rules.size());
}

// ---- replies ------------------------------------------------------------------------

namespace {

using core::replyQuoted;

std::string classRecords(const std::map<std::string, std::size_t>& classes)
{
    std::string out;
    for (const auto& [name, count] : classes) {
        out += std::format("\nclass name={} count={}", name, count);
    }
    return out;
}

std::string warningRecords(const std::vector<std::string>& warnings)
{
    std::string out;
    for (const std::string& warning : warnings) {
        out += "\n" + warningRecord(warning);
    }
    return out;
}

// Metres to the millimetre, the precision a survey states a position to.
std::string extentRecord(const geometry::Box2& bounds)
{
    if (bounds.empty()) {
        return {};
    }
    return std::format("\nextent min_x={:.3f} min_y={:.3f} max_x={:.3f} max_y={:.3f}", bounds.min.x,
                       bounds.min.y, bounds.max.x, bounds.max.y);
}

// What a read found, the head of both an import's reply and a description.
std::string readFields(const IfcImport& read, std::string_view fileName, std::size_t entities)
{
    return std::format("file={} schema={} crs={} entities={} alignments={} surfaces={} "
                       "objects={} drawn={} as_points={} alignments_as_polylines={}",
                       replyQuoted(fileName), read.schema, replyQuoted(read.coordinateSystem),
                       entities, read.alignments.size(), read.surfaces.size(), read.products,
                       read.productsImported, read.productsAsPoints, read.alignmentsAsPolylines);
}

} // namespace

std::string noteRecord(std::string_view text)
{
    return "note text=" + replyQuoted(text);
}

std::string warningRecord(std::string_view text)
{
    return "warning text=" + replyQuoted(text);
}

std::string formatExportReply(const IfcExport& report, std::string_view fileName, bool preview)
{
    std::string out =
        preview ? std::format("ifc previewed file={} schema=IFC4X3_ADD2 instances={}",
                              replyQuoted(fileName), report.instances)
                : std::format("ifc exported file={} schema=IFC4X3_ADD2 instances={} bytes={}",
                              replyQuoted(fileName), report.instances, report.bytesWritten);
    out += std::format("\ncounts alignments={} services={} segments={} segments_3d={} "
                       "located_points={} entities_written={} entities_skipped={} surfaces={}",
                       report.alignments, report.services, report.serviceSegments,
                       report.segmentsIn3d, report.locatedPoints, report.entitiesWritten,
                       report.entitiesSkipped, report.surfaces);
    out += classRecords(report.classes);
    for (const ClassTally& row : report.tally) {
        out += std::format("\nobject from={} count={} class={} predefined={} object_type={} "
                           "system={} why={}",
                           replyQuoted(row.source), row.count, replyQuoted(row.entity),
                           replyQuoted(row.predefinedType), replyQuoted(row.objectType),
                           replyQuoted(row.system), replyQuoted(row.why));
    }
    return out + warningRecords(report.warnings);
}

std::string formatImportReply(const IfcImport& imported, std::string_view fileName,
                              std::size_t entities)
{
    return "ifc imported " + readFields(imported, fileName, entities) +
           classRecords(imported.classes) + extentRecord(imported.bounds) +
           warningRecords(imported.warnings);
}

std::string formatDescription(const IfcImport& read, std::string_view fileName)
{
    std::string out = "ifc described " + readFields(read, fileName, read.entities.size()) +
                      std::format(" layers={}", read.layers.size()) + classRecords(read.classes);
    for (const auto& alignment : read.alignments) {
        out += std::format("\nalignment name={} pis={} pvis={}", replyQuoted(alignment.name),
                           alignment.horizontal.pis.size(),
                           alignment.vertical ? alignment.vertical->pvis.size() : 0);
    }
    for (const auto& surface : read.surfaces) {
        out += std::format("\nsurface name={} triangles={}", replyQuoted(surface.name),
                           surface.surface.triangleCount());
    }
    return out + extentRecord(read.bounds) + warningRecords(read.warnings);
}

Result<std::vector<ClassTally>> readExportObjects(std::string_view reply)
{
    std::vector<ClassTally> out;
    std::size_t number = 0;
    for (const std::string_view line : core::splitLines(reply)) {
        ++number;
        const auto record = core::readReplyRecord(line);
        if (!record || record->words != std::vector<std::string>{"object"}) {
            continue;
        }
        const auto count = record->value("count");
        const auto parsed = count ? core::parseInteger(*count) : std::nullopt;
        if (!parsed || *parsed < 0 || !record->value("from") || !record->value("class")) {
            return makeError(ErrorCode::ParseFailure, "an object record does not read",
                             std::format("line {}", number));
        }
        ClassTally row;
        row.source = *record->value("from");
        row.count = static_cast<std::size_t>(*parsed);
        row.entity = *record->value("class");
        row.predefinedType = record->value("predefined").value_or("");
        row.objectType = record->value("object_type").value_or("");
        row.system = record->value("system").value_or("");
        row.why = record->value("why").value_or("");
        out.push_back(std::move(row));
    }
    return out;
}

std::string guidNamespaceFor(std::string_view projectName, std::string_view createdUtc)
{
    return std::string(projectName) + "/" + std::string(createdUtc);
}

std::string headerTimestamp(std::chrono::system_clock::time_point when)
{
    return std::format("{:%Y-%m-%dT%H:%M:%S}", std::chrono::floor<std::chrono::seconds>(when));
}

} // namespace katana::ifc
