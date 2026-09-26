// The geoprocessing verbs' reply records (replies.hpp).

#include "replies.hpp"

#include <algorithm>
#include <charconv>
#include <cstdint>
#include <iterator>
#include <optional>
#include <string>
#include <vector>

#include "katana/cad/scope_verbs.hpp"
#include "katana/core/text.hpp"

namespace katana::app::geo {

namespace gp = katana::gis::processing;

namespace {

std::string joined(const std::vector<std::string>& items, std::string_view separator)
{
    std::string text;
    for (const std::string& item : items) {
        text += (text.empty() ? "" : std::string(separator)) + item;
    }
    return text;
}

std::string yesNo(bool flag)
{
    return flag ? "yes" : "no";
}

} // namespace

std::string value(std::string_view text)
{
    // Nothing at all for nothing ("aliases="), so an absent value is not a
    // pair of quotes a reader has to recognise.
    return text.empty() ? std::string() : katana::cad::recordValue(text);
}

bool lineKeyword(std::string_view word)
{
    // Compared lower case (core::lowered): a line reads its words in any case.
    static constexpr std::string_view kKeywords[] = {
        // Clauses and flags.
        "to", "from", "preview", "confirm", "overwrite", "replace", "append", "convex", "holes",
        "observers", "only", "extents", "drawn",
        // Words that take a value.
        "name", "save", "format", "layer", "layers", "with", "by", "minus", "at", "as", "observer",
        "target", "cell", "where",
        // Sources, targets and scopes.
        "file", "raster", "surface", "reference", "report", "selection", "sel", "drawing", "all",
        "view", "area"};
    const std::string folded = katana::core::lowered(word);
    return std::ranges::find(kKeywords, std::string_view(folded)) != std::end(kKeywords);
}

std::optional<std::string> lineWord(std::string_view text)
{
    if (text.find_first_of("\"\r\n") != std::string_view::npos) {
        return std::nullopt;
    }
    const bool quote =
        text.empty() || text.find_first_of(" \t") != std::string_view::npos || lineKeyword(text);
    return quote ? "\"" + std::string(text) + "\"" : std::string(text);
}

std::string fixed3(double number)
{
    char buffer[64];
    const auto [end, error] =
        std::to_chars(buffer, buffer + sizeof(buffer), number, std::chars_format::fixed, 3);
    return error == std::errc() ? std::string(buffer, end) : std::string("nan");
}

std::string gdalRecord(const gp::AlgorithmInfo& info, double seconds)
{
    return "gdal algorithm=" + value(gp::pathText(info.path)) +
           " policy=" + std::string(gp::toString(info.policy)) + " seconds=" + fixed3(seconds) +
           " cancelled=no";
}

std::string algorithmRecord(const gp::AlgorithmInfo& info)
{
    return "algorithm path=" + value(gp::pathText(info.path)) +
           " policy=" + std::string(gp::toString(info.policy)) +
           " aliases=" + value(joined(info.aliases, ",")) +
           " description=" + value(info.description);
}

std::string groupRecord(const gp::AlgorithmInfo& info, std::size_t algorithms)
{
    return "group path=" + value(gp::pathText(info.path)) +
           " algorithms=" + std::to_string(algorithms) + " description=" + value(info.description);
}

std::string argRecord(const gp::ArgSpec& arg)
{
    std::string record = "arg name=" + value(arg.name) + " short=" + value(arg.shortName) +
                         " aliases=" + value(joined(arg.aliases, ",")) +
                         " type=" + std::string(gp::toString(arg.type)) +
                         " required=" + yesNo(arg.required) +
                         " positional=" + yesNo(arg.positional) + " category=" + value(arg.category) +
                         " default=" + value(arg.defaultValue ? gp::toString(*arg.defaultValue) : "");
    record += " min=" + (arg.min ? katana::core::formatExactReal(arg.min->value) : std::string()) +
              " min_inclusive=" + (arg.min ? yesNo(arg.min->inclusive) : std::string());
    record += " max=" + (arg.max ? katana::core::formatExactReal(arg.max->value) : std::string()) +
              " max_inclusive=" + (arg.max ? yesNo(arg.max->inclusive) : std::string());
    record += " choices=" + value(joined(arg.choices, ","));
    record += " count=" + std::to_string(arg.minCount) + ".." +
              (arg.maxCount < 0 ? std::string("*") : std::to_string(arg.maxCount));
    std::vector<std::string> accepts;
    if (arg.acceptsName) {
        accepts.emplace_back("name");
    }
    if (arg.acceptsObject) {
        accepts.emplace_back("object");
    }
    record += " dataset=" + gp::datasetKindsText(arg.datasetKinds) +
              " accepts=" + joined(accepts, ",") + " input=" + yesNo(arg.isInput) +
              " output=" + yesNo(arg.isOutput) + " description=" + value(arg.description);
    return record;
}

std::string sourcesFor(const gp::ArgSpec& arg)
{
    std::vector<std::string> sources;
    if ((arg.datasetKinds & gp::DatasetKind::Vector) != 0) {
        sources.emplace_back("drawing");
    }
    if ((arg.datasetKinds & gp::DatasetKind::Raster) != 0) {
        sources.emplace_back("raster");
        sources.emplace_back("surface");
    }
    sources.emplace_back("file");
    return joined(sources, ",");
}

std::string bindingRecord(const gp::ArgSpec& arg)
{
    std::vector<std::string> accepts;
    if (arg.acceptsName) {
        accepts.emplace_back("name");
    }
    if (arg.acceptsObject) {
        accepts.emplace_back("object");
    }
    return "binding arg=" + value(arg.name) + " kinds=" + gp::datasetKindsText(arg.datasetKinds) +
           " accepts=" + joined(accepts, ",") + " sources=" + sourcesFor(arg) +
           " list=" + yesNo(arg.type == gp::ArgType::DatasetList) +
           " required=" + yesNo(arg.required);
}

std::string versionRecord(const gp::Versions& versions)
{
    return "gdal version=" + value(versions.gdal) + " release=" + value(versions.release) +
           " proj=" + value(versions.proj) + " geos=" + value(versions.geos) +
           " raster_drivers=" + std::to_string(versions.rasterDrivers) +
           " vector_drivers=" + std::to_string(versions.vectorDrivers) +
           " algorithms=" + std::to_string(versions.algorithms);
}

std::string textRecord(std::string_view text)
{
    std::string body(text);
    while (!body.empty() && (body.back() == '\n' || body.back() == '\r')) {
        body.pop_back();
    }
    const std::vector<std::string_view> lines = katana::core::splitLines(body);
    std::string record = "text lines=" + std::to_string(lines.size());
    for (const std::string_view line : lines) {
        record += "\n" + std::string(line);
    }
    return record;
}

std::string warningRecord(std::string_view text)
{
    return "warning text=" + value(text);
}

std::optional<std::string> Record::get(std::string_view key) const
{
    for (const auto& [name, text] : fields) {
        if (name == key) {
            return text;
        }
    }
    return std::nullopt;
}

std::vector<Record> parseRecords(std::string_view text)
{
    const std::vector<std::string_view> lines = katana::core::splitLines(text);
    std::vector<Record> records;
    for (std::size_t n = 0; n < lines.size(); ++n) {
        const std::string_view line = lines[n];
        Record record;
        // The kind is every word before the first key=: IFC's records have
        // two ("ifc exported file=..."), and reading one word made the
        // second part of the first key ("exported file"). A bare word after
        // the fields is a field with no value, not glued to the next key.
        std::size_t i = 0;
        bool fieldsBegun = false;
        while (i != std::string_view::npos && i < line.size()) {
            while (i < line.size() && line[i] == ' ') {
                ++i;
            }
            if (i >= line.size()) {
                break;
            }
            const std::size_t equals = line.find('=', i);
            const std::size_t space = line.find(' ', i);
            if (equals == std::string_view::npos ||
                (space != std::string_view::npos && space < equals)) {
                std::string word(
                    line.substr(i, space == std::string_view::npos ? space : space - i));
                if (fieldsBegun) {
                    record.fields.emplace_back(std::move(word), std::string());
                } else {
                    record.kind += (record.kind.empty() ? "" : " ") + word;
                }
                i = space;
                continue;
            }
            fieldsBegun = true;
            std::string key(line.substr(i, equals - i));
            std::string field;
            i = equals + 1;
            if (i < line.size() && line[i] == '"') {
                // recordValue's quoting: a quote and a backslash escaped with a
                // backslash.
                for (++i; i < line.size() && line[i] != '"'; ++i) {
                    if (line[i] == '\\' && i + 1 < line.size()) {
                        ++i;
                    }
                    field += line[i];
                }
                ++i;
            } else {
                const std::size_t end = line.find(' ', i);
                field = std::string(line.substr(i, end == std::string_view::npos ? end : end - i));
                i = end;
            }
            record.fields.emplace_back(std::move(key), std::move(field));
        }
        if (record.kind == "text") {
            const auto count = katana::core::parseInteger(record.get("lines").value_or("0"));
            for (std::int64_t k = 0; count && k < *count && n + 1 < lines.size(); ++k) {
                record.body.emplace_back(lines[++n]);
            }
        }
        records.push_back(std::move(record));
    }
    return records;
}

} // namespace katana::app::geo
