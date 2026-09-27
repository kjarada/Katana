// FORMATS (formats_verbs.hpp, docs/interop.md "Formats"): what this build of
// GDAL reads and writes, and what each driver's options are, from GDAL's own
// registry. It changes nothing, so it answers at prepare, as GDAL LIST does.

#include "formats_verbs.hpp"

#include <algorithm>
#include <string>
#include <utility>
#include <vector>

#include "katana/core/text.hpp"
#include "katana/gis/processing.hpp"
#include "replies.hpp"
#include "verb_table.hpp"

namespace katana::app::geo {

namespace gis = katana::gis;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;

namespace {

constexpr const char* kUsage =
    "FORMATS [RASTER|VECTOR] [READ|WRITE] [<text>...] [JSON] | FORMATS OPTIONS <driver> [JSON]";

std::string joined(const std::vector<std::string>& items)
{
    std::string text;
    for (const std::string& item : items) {
        text += (text.empty() ? "" : ",") + item;
    }
    return text;
}

std::vector<std::string> kinds(bool raster, bool vector)
{
    std::vector<std::string> out;
    if (raster) {
        out.emplace_back("raster");
    }
    if (vector) {
        out.emplace_back("vector");
    }
    return out;
}

// "raster,vector", or "no" where it does neither: a record never has a
// value a reader must know means nothing.
std::string kindsOrNo(bool raster, bool vector)
{
    const std::string text = joined(kinds(raster, vector));
    return text.empty() ? std::string("no") : text;
}

const char* capabilityWord(FormatsQuery::Capability capability)
{
    switch (capability) {
    case FormatsQuery::Capability::Read:
        return "read";
    case FormatsQuery::Capability::Write:
        return "write";
    case FormatsQuery::Capability::Any:
        break;
    }
    return "any";
}

Prepared answered(std::string reply)
{
    Prepared prepared;
    prepared.title = "FORMATS";
    prepared.reply = std::move(reply);
    return prepared;
}

std::string lines(const std::vector<std::string>& records)
{
    std::string text;
    for (const std::string& record : records) {
        text += (text.empty() ? "" : "\n") + record;
    }
    return text;
}

Result<Prepared> options(const Tokens& tokens, std::size_t at)
{
    bool json = false;
    std::string driver;
    for (; at < tokens.size(); ++at) {
        if (tokens.is(at, "JSON")) {
            json = true;
        } else if (driver.empty()) {
            driver = tokens[at];
        } else {
            // "ESRI Shapefile": a driver name of two words, unquoted.
            driver += " " + tokens[at];
        }
    }
    if (driver.empty()) {
        return makeError(ErrorCode::InvalidArgument,
                         "FORMATS OPTIONS names a driver: " + std::string(kUsage));
    }
    const gis::Format* format = gis::findFormat(driver);
    if (format == nullptr) {
        return makeError(ErrorCode::NotFound,
                         "this GDAL build has no format of that name (FORMATS lists them)",
                         driver);
    }
    auto found = gis::formatOptions(format->driver);
    if (!found) {
        return found.error();
    }
    if (json) {
        return answered(formatOptionsJson(*format, *found).dump());
    }
    return answered(optionsRecords(*format, *found) + "\nlisted driver=" + value(format->driver) +
                    " open=" + std::to_string(found->open.size()) +
                    " creation=" + std::to_string(found->creation.size()) +
                    " layer_creation=" + std::to_string(found->layerCreation.size()));
}

} // namespace

std::vector<const gis::Format*> chooseFormats(const FormatsQuery& query)
{
    std::vector<std::string> words;
    {
        std::string word;
        for (const char c : katana::core::lowered(query.text)) {
            if (c == ' ' || c == '\t') {
                if (!word.empty()) {
                    words.push_back(std::move(word));
                    word.clear();
                }
            } else {
                word += c;
            }
        }
        if (!word.empty()) {
            words.push_back(std::move(word));
        }
    }
    std::vector<const gis::Format*> chosen;
    for (const gis::Format& format : gis::formats()) {
        const bool raster = !query.kind || *query.kind == gis::DataKind::Raster;
        const bool vector = !query.kind || *query.kind == gis::DataKind::Vector;
        bool take = false;
        switch (query.capability) {
        case FormatsQuery::Capability::Any:
            take = (raster && format.raster) || (vector && format.vector);
            break;
        case FormatsQuery::Capability::Read:
            take = (raster && format.readRaster) || (vector && format.readVector);
            break;
        case FormatsQuery::Capability::Write:
            take = (raster && format.writeRaster) || (vector && format.writeVector);
            break;
        }
        if (!take) {
            continue;
        }
        const std::string haystack = katana::core::lowered(
            format.driver + " " + format.description + " " + joined(format.extensions));
        const bool matches = std::ranges::all_of(words, [&haystack](const std::string& word) {
            return haystack.find(word) != std::string::npos;
        });
        if (matches) {
            chosen.push_back(&format);
        }
    }
    return chosen;
}

std::string formatRecord(const gis::Format& format)
{
    return "format driver=" + value(format.driver) +
           " kind=" + kindsOrNo(format.raster, format.vector) +
           " read=" + kindsOrNo(format.readRaster, format.readVector) +
           " write=" + kindsOrNo(format.writeRaster, format.writeVector) +
           " extensions=" + value(joined(format.extensions)) +
           " vsi=" + (format.virtualIo ? "yes" : "no") +
           " description=" + value(format.description);
}

std::string formatsSummary(const FormatsQuery& query, std::size_t count)
{
    const char* kind = !query.kind                               ? "any"
                       : *query.kind == gis::DataKind::Raster ? "raster"
                                                              : "vector";
    return "listed formats=" + std::to_string(count) + " kind=" + kind +
           " capability=" + capabilityWord(query.capability) + " filter=" + value(query.text) +
           " gdal=" + value(gis::processing::versions().gdal);
}

nlohmann::json formatJson(const gis::Format& format)
{
    return nlohmann::json{
        {"driver", format.driver},
        {"description", format.description},
        {"kinds", kinds(format.raster, format.vector)},
        {"read", kinds(format.readRaster, format.readVector)},
        {"write", kinds(format.writeRaster, format.writeVector)},
        {"extensions", format.extensions},
        {"vsi", format.virtualIo},
        {"connection_prefix",
         format.connectionPrefix.empty() ? nlohmann::json() : nlohmann::json(format.connectionPrefix)},
        {"help_url", format.helpUrl.empty() ? nlohmann::json() : nlohmann::json(format.helpUrl)},
    };
}

nlohmann::json formatOptionJson(const gis::FormatOption& option)
{
    const auto text = [](const std::string& value) {
        return value.empty() ? nlohmann::json() : nlohmann::json(value);
    };
    return nlohmann::json{
        {"name", option.name},
        {"type", text(option.type)},
        {"description", text(option.description)},
        {"default", text(option.defaultValue)},
        {"scope", text(option.scope)},
        {"choices", option.choices},
        {"min", option.min ? nlohmann::json(*option.min) : nlohmann::json()},
        {"max", option.max ? nlohmann::json(*option.max) : nlohmann::json()},
    };
}

std::string optionRecord(const gis::Format& format, std::string_view list,
                         const gis::FormatOption& option)
{
    const auto bound = [](const std::optional<double>& number) {
        return number ? katana::core::formatExactReal(*number) : std::string();
    };
    return "option driver=" + value(format.driver) + " list=" + std::string(list) +
           " name=" + value(option.name) + " type=" + value(option.type) +
           " default=" + value(option.defaultValue) + " scope=" + value(option.scope) +
           " choices=" + value(joined(option.choices)) + " min=" + bound(option.min) +
           " max=" + bound(option.max) + " description=" + value(option.description);
}

std::string optionsRecords(const gis::Format& format, const gis::FormatOptions& options)
{
    std::string text = formatRecord(format);
    for (const auto& [list, entries] :
         {std::pair{"open", &options.open}, std::pair{"creation", &options.creation},
          std::pair{"layer_creation", &options.layerCreation}}) {
        for (const gis::FormatOption& option : *entries) {
            text += "\n" + optionRecord(format, list, option);
        }
    }
    return text;
}

nlohmann::json formatOptionsJson(const gis::Format& format, const gis::FormatOptions& options)
{
    const auto list = [](const std::vector<gis::FormatOption>& entries) {
        nlohmann::json array = nlohmann::json::array();
        for (const gis::FormatOption& option : entries) {
            array.push_back(formatOptionJson(option));
        }
        return array;
    };
    return nlohmann::json{{"format", formatJson(format)},
                          {"open_options", list(options.open)},
                          {"creation_options", list(options.creation)},
                          {"layer_creation_options", list(options.layerCreation)}};
}

// ---- I2: the verb ------------------------------------------------------------------------------

Result<Prepared> prepareFormats(Context&, const Tokens& tokens, std::string_view)
{
    if (tokens.size() > 1 && tokens.is(1, "OPTIONS")) {
        return options(tokens, 2);
    }
    FormatsQuery query;
    bool json = false;
    std::vector<std::string> text;
    for (std::size_t at = 1; at < tokens.size(); ++at) {
        if (tokens.is(at, "JSON")) {
            json = true;
        } else if (tokens.is(at, "RASTER") || tokens.is(at, "VECTOR")) {
            const gis::DataKind kind =
                tokens.is(at, "RASTER") ? gis::DataKind::Raster : gis::DataKind::Vector;
            if (query.kind && *query.kind != kind) {
                return makeError(ErrorCode::InvalidArgument,
                                 "FORMATS takes RASTER or VECTOR, not both; neither lists both: " +
                                     std::string(kUsage));
            }
            query.kind = kind;
        } else if (tokens.is(at, "READ") || tokens.is(at, "WRITE")) {
            const auto capability = tokens.is(at, "READ") ? FormatsQuery::Capability::Read
                                                          : FormatsQuery::Capability::Write;
            if (query.capability != FormatsQuery::Capability::Any &&
                query.capability != capability) {
                return makeError(ErrorCode::InvalidArgument,
                                 "FORMATS takes READ or WRITE, not both: " + std::string(kUsage));
            }
            query.capability = capability;
        } else {
            text.push_back(tokens[at]);
        }
    }
    for (const std::string& word : text) {
        query.text += (query.text.empty() ? "" : " ") + word;
    }
    const std::vector<const gis::Format*> chosen = chooseFormats(query);
    if (json) {
        nlohmann::json array = nlohmann::json::array();
        for (const gis::Format* format : chosen) {
            array.push_back(formatJson(*format));
        }
        return answered(array.dump());
    }
    std::vector<std::string> records;
    for (const gis::Format* format : chosen) {
        records.push_back(formatRecord(*format));
    }
    records.push_back(formatsSummary(query, chosen.size()));
    return answered(lines(records));
}

} // namespace katana::app::geo
