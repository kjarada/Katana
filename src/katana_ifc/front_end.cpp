#include "katana/ifc/front_end.hpp"

#include <format>
#include <fstream>
#include <iterator>

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
// first ".ifc" that ends a word. nullopt when there is no .ifc.
std::optional<std::pair<std::string, std::string_view>> splitPath(std::string_view argument)
{
    argument = core::trimmed(argument);
    if (argument.starts_with('"')) {
        const std::size_t end = argument.find('"', 1);
        if (end == std::string_view::npos) {
            return std::nullopt;
        }
        std::string path(argument.substr(1, end - 1));
        if (!isIfcPath(pathFromUtf8(path))) {
            return std::nullopt;
        }
        return std::pair(std::move(path), argument.substr(end + 1));
    }
    const std::string lower = core::lowered(argument);
    for (std::size_t at = lower.find(".ifc"); at != std::string::npos;
         at = lower.find(".ifc", at + 1)) {
        const std::size_t end = at + 4;
        if (end == lower.size() || lower[end] == ' ' || lower[end] == '\t') {
            return std::pair(std::string(argument.substr(0, end)), argument.substr(end));
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
    return std::filesystem::path(std::u8string(text.begin(), text.end()));
}

std::optional<Result<ExportArguments>> parseExportArguments(std::string_view argument)
{
    const auto split = splitPath(argument);
    if (!split) {
        return std::nullopt;
    }
    ExportArguments out;
    out.path = split->first;
    const auto keywords = words(split->second);
    if (!keywords) {
        return Result<ExportArguments>(invalid("a quoted path is never closed"));
    }
    for (std::size_t i = 0; i < keywords->size(); ++i) {
        const std::string keyword = upper((*keywords)[i]);
        const bool hasValue = i + 1 < keywords->size();
        if (keyword == "UTILITIES" && hasValue) {
            out.schedule = (*keywords)[++i];
        } else if (keyword == "SCHEMA" && hasValue) {
            out.schema = (*keywords)[++i];
        } else if (keyword == "RULES" && hasValue) {
            out.rules = (*keywords)[++i];
        } else if (keyword == "SPACING" && hasValue) {
            out.spacing = core::parseFiniteDouble((*keywords)[++i]);
            if (!out.spacing || *out.spacing <= 0.0) {
                return Result<ExportArguments>(
                    invalid("SPACING takes a positive number of metres"));
            }
        } else if (keyword == "NODRAWING") {
            out.drawing = false;
        } else {
            return Result<ExportArguments>(
                invalid("\"" + (*keywords)[i] +
                        "\" is not an option of EXPORT <file.ifc>: UTILITIES <schedule.csv>, "
                        "SCHEMA <schema.csv>, RULES <rules.csv>, SPACING <m>, NODRAWING"));
        }
    }
    if (out.schema && !out.schedule) {
        return Result<ExportArguments>(
            invalid("SCHEMA describes a schedule: give it with UTILITIES"));
    }
    return Result<ExportArguments>(std::move(out));
}

std::optional<Result<ImportArguments>> parseImportArguments(std::string_view argument)
{
    const auto split = splitPath(argument);
    if (!split) {
        return std::nullopt;
    }
    ImportArguments out;
    out.path = split->first;
    const auto keywords = words(split->second);
    if (!keywords) {
        return Result<ImportArguments>(invalid("a quoted path is never closed"));
    }
    for (const std::string& keyword : *keywords) {
        if (upper(keyword) == "LOCAL") {
            out.local = true;
        } else {
            return Result<ImportArguments>(
                invalid("\"" + keyword + "\" is not an option of IMPORT <file.ifc>: LOCAL"));
        }
    }
    return Result<ImportArguments>(std::move(out));
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

std::string guidNamespaceFor(std::string_view projectName, std::string_view createdUtc)
{
    return std::string(projectName) + "/" + std::string(createdUtc);
}

std::string headerTimestamp(std::chrono::system_clock::time_point when)
{
    return std::format("{:%Y-%m-%dT%H:%M:%S}", std::chrono::floor<std::chrono::seconds>(when));
}

} // namespace katana::ifc
