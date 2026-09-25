#include "katana/interop/online_verbs.hpp"

#include <cmath>

#include "katana/core/text.hpp"
#include "katana/gis/web_access.hpp"
#include "katana/interop/online_requests.hpp"

namespace katana::interop {
namespace {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;

std::string upper(std::string_view text)
{
    std::string out(text);
    for (char& c : out) {
        if (c >= 'a' && c <= 'z') {
            c = static_cast<char>(c - 'a' + 'A');
        }
    }
    return out;
}

// Words separated by whitespace, a pair of double quotes keeping spaces in
// one word ("layer=\"my layer\"" and "\"a b\"" alike) and removed.
Result<std::vector<std::string>> words(std::string_view line)
{
    std::vector<std::string> out;
    std::string word;
    bool quoted = false;
    bool any = false;
    for (const char c : line) {
        if (c == '"') {
            quoted = !quoted;
            any = true;
            continue;
        }
        if (!quoted && (c == ' ' || c == '\t')) {
            if (any) {
                out.push_back(word);
            }
            word.clear();
            any = false;
            continue;
        }
        word.push_back(c);
        any = true;
    }
    if (quoted) {
        return makeError(ErrorCode::InvalidArgument, "a quoted value is never closed");
    }
    if (any) {
        out.push_back(word);
    }
    return out;
}

bool validDate(const std::string& text)
{
    if (text.size() != 10 || text[4] != '-' || text[7] != '-') {
        return false;
    }
    for (const std::size_t i : {0u, 1u, 2u, 3u, 5u, 6u, 8u, 9u}) {
        if (text[i] < '0' || text[i] > '9') {
            return false;
        }
    }
    const int month = std::stoi(text.substr(5, 2));
    const int day = std::stoi(text.substr(8, 2));
    return month >= 1 && month <= 12 && day >= 1 && day <= 31;
}

std::optional<double> number(std::string_view text)
{
    return katana::core::parseFiniteDouble(katana::core::trimmed(text));
}

Result<katana::gis::CrsBox> parseBox(std::string_view text)
{
    std::vector<double> values;
    std::size_t start = 0;
    while (start <= text.size()) {
        std::size_t end = text.find(',', start);
        if (end == std::string_view::npos) {
            end = text.size();
        }
        const auto value = number(text.substr(start, end - start));
        if (!value) {
            return makeError(ErrorCode::InvalidArgument,
                             "the area is four numbers, x0,y0,x1,y1", std::string(text));
        }
        values.push_back(*value);
        start = end + 1;
        if (end == text.size()) {
            break;
        }
    }
    if (values.size() != 4) {
        return makeError(ErrorCode::InvalidArgument, "the area is four numbers, x0,y0,x1,y1",
                         std::string(text));
    }
    // Corners in either order: x0,y0 and x1,y1 are two corners of the box.
    katana::gis::CrsBox box{std::min(values[0], values[2]), std::min(values[1], values[3]),
                            std::max(values[0], values[2]), std::max(values[1], values[3])};
    if (!box.valid()) {
        return makeError(ErrorCode::InvalidArgument, "the area has no width or no height",
                         std::string(text));
    }
    return box;
}

std::string usageOf(OnlineVerb verb)
{
    switch (verb) {
    case OnlineVerb::Providers:
        return "ONLINE PROVIDERS [filter]";
    case OnlineVerb::Layers:
        return "ONLINE LAYERS <provider> [search words]";
    case OnlineVerb::Info:
        return "ONLINE INFO <provider> <layer>";
    case OnlineVerb::Import:
        return "ONLINE IMPORT <provider> <layer> area=view|drawing|selection|x0,y0,x1,y1|"
               "lonlat:w,s,e,n [res=] [layer=] [from=YYYY-MM-DD] [to=YYYY-MM-DD] [cloud=] "
               "[tag=] [time=] [crs=EPSG:<code>] [timeout=<seconds>]";
    case OnlineVerb::Custom:
        return "ONLINE CUSTOM <url>";
    case OnlineVerb::Key:
        return "ONLINE KEY <name> [value]";
    }
    return {};
}

std::string field(const char* name, std::string_view value)
{
    return std::string(" ") + name + "=" + replyValue(value);
}

std::string coverageText(const LonLatBox& box)
{
    return formatNumber(box[0]) + "," + formatNumber(box[1]) + "," + formatNumber(box[2]) + "," +
           formatNumber(box[3]);
}

} // namespace

const char* toString(OnlineVerb verb)
{
    switch (verb) {
    case OnlineVerb::Providers:
        return "PROVIDERS";
    case OnlineVerb::Layers:
        return "LAYERS";
    case OnlineVerb::Info:
        return "INFO";
    case OnlineVerb::Import:
        return "IMPORT";
    case OnlineVerb::Custom:
        return "CUSTOM";
    case OnlineVerb::Key:
        return "KEY";
    }
    return "?";
}

std::string onlineUsage()
{
    std::string text;
    for (const OnlineVerb verb : {OnlineVerb::Providers, OnlineVerb::Layers, OnlineVerb::Info,
                                  OnlineVerb::Import, OnlineVerb::Custom, OnlineVerb::Key}) {
        text += "usage: " + usageOf(verb) + "\n";
    }
    return text;
}

std::string replyValue(std::string_view value)
{
    const bool plain = !value.empty() && value.find_first_of(" \t\"=\\\n\r") == std::string_view::npos;
    if (plain) {
        return std::string(value);
    }
    std::string out = "\"";
    for (const char c : value) {
        if (c == '"' || c == '\\') {
            out.push_back('\\');
        }
        out.push_back(c == '\n' || c == '\r' ? ' ' : c);
    }
    return out + "\"";
}

Result<OnlineCommand> parseOnlineCommand(std::string_view line)
{
    auto parts = words(line);
    if (!parts) {
        return parts.error();
    }
    if (parts->empty() || upper(parts->front()) != "ONLINE") {
        return makeError(ErrorCode::InvalidArgument, "not an ONLINE command", std::string(line));
    }
    if (parts->size() < 2) {
        return makeError(ErrorCode::InvalidArgument, onlineUsage());
    }
    const std::string verb = upper((*parts)[1]);
    const std::vector<std::string> rest(parts->begin() + 2, parts->end());
    OnlineCommand command;
    const auto refuse = [&](const std::string& why, OnlineVerb which) {
        return makeError(ErrorCode::InvalidArgument, why, "usage: " + usageOf(which));
    };
    if (verb == "PROVIDERS") {
        command.verb = OnlineVerb::Providers;
        for (const std::string& word : rest) {
            command.filter += (command.filter.empty() ? "" : " ") + word;
        }
        return command;
    }
    if (verb == "LAYERS") {
        command.verb = OnlineVerb::Layers;
        if (rest.empty()) {
            return refuse("which provider's layers?", command.verb);
        }
        command.provider = rest[0];
        for (std::size_t i = 1; i < rest.size(); ++i) {
            command.filter += (command.filter.empty() ? "" : " ") + rest[i];
        }
        return command;
    }
    if (verb == "INFO") {
        command.verb = OnlineVerb::Info;
        if (rest.size() != 2) {
            return refuse("INFO names a provider and a layer", command.verb);
        }
        command.provider = rest[0];
        command.layer = rest[1];
        return command;
    }
    if (verb == "CUSTOM") {
        command.verb = OnlineVerb::Custom;
        if (rest.size() != 1) {
            return refuse("CUSTOM takes one URL", command.verb);
        }
        command.url = rest[0];
        return command;
    }
    if (verb == "KEY") {
        command.verb = OnlineVerb::Key;
        if (rest.empty() || rest.size() > 2) {
            return refuse("KEY takes a name and a value", command.verb);
        }
        command.keyName = rest[0];
        command.keyValue = rest.size() == 2 ? rest[1] : std::string();
        return command;
    }
    if (verb != "IMPORT") {
        return makeError(ErrorCode::InvalidArgument, "unknown ONLINE verb " + verb, onlineUsage());
    }
    command.verb = OnlineVerb::Import;
    std::vector<std::string> positional;
    bool sawArea = false;
    for (const std::string& word : rest) {
        const std::size_t equals = word.find('=');
        if (equals == std::string::npos) {
            positional.push_back(word);
            continue;
        }
        const std::string key = katana::core::lowered(word.substr(0, equals));
        const std::string value = word.substr(equals + 1);
        if (key == "area") {
            sawArea = true;
            const std::string folded = katana::core::lowered(value);
            if (folded == "view") {
                command.area = OnlineAreaKind::View;
            } else if (folded == "drawing" || folded == "extents") {
                command.area = OnlineAreaKind::Drawing;
            } else if (folded == "selection") {
                command.area = OnlineAreaKind::Selection;
            } else {
                std::string_view corners = value;
                for (const char* prefix : {"lonlat:", "wgs84:"}) {
                    if (folded.starts_with(prefix)) {
                        command.boxIsLonLat = true;
                        corners.remove_prefix(std::string_view(prefix).size());
                    }
                }
                auto box = parseBox(corners);
                if (!box) {
                    return box.error();
                }
                command.area = OnlineAreaKind::Box;
                command.box = *box;
                if (command.boxIsLonLat &&
                    (box->minX < -180.0 || box->maxX > 180.0 || box->minY < -90.0 || box->maxY > 90.0)) {
                    return makeError(ErrorCode::InvalidArgument,
                                     "a lonlat: area is longitudes within -180..180 and latitudes "
                                     "within -90..90",
                                     value);
                }
            }
        } else if (key == "res" || key == "resolution") {
            const auto parsed = number(value);
            if (!parsed || !(*parsed > 0.0)) {
                return refuse("res is a positive number of project units per pixel", command.verb);
            }
            command.resolution = *parsed;
        } else if (key == "layer") {
            if (value.empty()) {
                return refuse("layer= needs a name", command.verb);
            }
            command.targetLayer = value;
        } else if (key == "from" || key == "to") {
            if (!validDate(value)) {
                return refuse(key + " is a date, YYYY-MM-DD", command.verb);
            }
            (key == "from" ? command.fromDate : command.toDate) = value;
        } else if (key == "cloud") {
            const auto parsed = number(value);
            if (!parsed || *parsed < 0.0 || *parsed > 100.0) {
                return refuse("cloud is a percentage, 0 to 100", command.verb);
            }
            command.maxCloud = *parsed;
        } else if (key == "tag") {
            command.tag = value;
        } else if (key == "time") {
            command.time = value;
        } else if (key == "crs") {
            command.crs = value;
        } else if (key == "timeout") {
            const auto parsed = number(value);
            if (!parsed || *parsed < 1.0 || *parsed > 86400.0) {
                return refuse("timeout is seconds, 1 to 86400", command.verb);
            }
            command.timeoutSeconds = static_cast<int>(*parsed);
        } else {
            return refuse("unknown option " + key + "=", command.verb);
        }
    }
    if (positional.size() != 2) {
        return refuse("IMPORT names a provider and a layer", command.verb);
    }
    if (!sawArea) {
        return refuse("IMPORT needs area=", command.verb);
    }
    if (!command.fromDate.empty() && !command.toDate.empty() && command.fromDate > command.toDate) {
        return refuse("from= is after to=", command.verb);
    }
    command.provider = positional[0];
    command.layer = positional[1];
    return command;
}

std::string formatProviders(const std::vector<const OnlineProvider*>& providers)
{
    std::string text = "providers count=" + std::to_string(providers.size()) + "\n";
    for (const OnlineProvider* provider : providers) {
        text += "provider" + field("id", provider->id) + field("group", provider->group) +
                field("layers", std::to_string(provider->layers().size())) +
                field("custom", provider->userDefined ? "yes" : "no") +
                field("title", provider->title) + "\n";
    }
    return text;
}

std::string formatLayers(const OnlineProvider& provider)
{
    const auto layers = provider.layers();
    std::string text = "layers" + field("provider", provider.id) +
                       field("count", std::to_string(layers.size())) + "\n";
    for (const OnlineLayer* layer : layers) {
        text += "layer" + field("provider", provider.id) + field("id", layer->id) +
                field("kind", toString(layer->kind)) + field("service", toString(layer->type)) +
                field("key", layer->keyRequired ? "yes" : "no") +
                field("verified", layer->verified.empty() ? "no" : layer->verified) +
                field("licence", layer->licence) + field("title", layer->title) + "\n";
    }
    return text;
}

std::string formatInfo(const OnlineLayer& layer)
{
    std::string text = "info" + field("provider", layer.providerId) + field("layer", layer.id) + "\n";
    const auto line = [&](const char* name, const std::string& value) {
        if (!value.empty()) {
            text += "field" + field("name", name) + field("value", value) + "\n";
        }
    };
    line("title", layer.title);
    line("provider_title", layer.providerTitle);
    line("group", layer.group);
    line("kind", toString(layer.kind));
    line("service", toString(layer.type));
    line("endpoint", katana::gis::redactUrl(layer.endpoint));
    line("service_layer", layer.layerName);
    line("version", layer.version);
    line("crs", layer.crs);
    line("coverage", coverageText(layer.coverage));
    line("resolution", layer.resolution ? formatNumber(*layer.resolution) : std::string());
    switch (layer.type) {
    case OnlineServiceType::XyzTiles:
    case OnlineServiceType::Wmts:
        line("max_zoom", std::to_string(layer.maxZoom));
        line("max_tiles", std::to_string(layer.maxTiles));
        break;
    case OnlineServiceType::ArcgisQuery:
    case OnlineServiceType::Wfs:
    case OnlineServiceType::OgcApiFeatures:
        line("page_size", std::to_string(layer.pageSize));
        break;
    case OnlineServiceType::ArcgisExport:
    case OnlineServiceType::ArcgisExportImage:
    case OnlineServiceType::Wms:
    case OnlineServiceType::Wcs:
        line("max_request_pixels", std::to_string(layer.maxRequestPixels));
        break;
    case OnlineServiceType::Overpass:
        line("max_area_km2", formatNumber(layer.maxAreaKm2));
        line("default_tag", layer.layerName);
        break;
    case OnlineServiceType::Stac:
        line("asset", layer.asset);
        line("max_cloud", formatNumber(layer.maxCloud));
        line("days", std::to_string(layer.days));
        break;
    default:
        break;
    }
    line("cache_days", std::to_string(layer.cacheDays));
    line("licence", layer.licence);
    line("attribution", layer.attribution);
    line("key", layer.keyRequired ? "yes (" + layer.keyName + ")" : "no");
    line("verified", layer.verified.empty() ? "no" : layer.verified);
    line("evidence", layer.evidence);
    line("homepage", layer.homepage);
    return text;
}

std::string formatImport(const OnlineLayer& layer, const OnlineImport& result,
                         std::size_t entitiesAdded, const std::string& targetLayer)
{
    std::string text = "imported" + field("provider", layer.providerId) + field("layer", layer.id) +
                       field("kind", toString(result.kind));
    if (result.raster) {
        text += field("width", std::to_string(result.raster->width)) +
                field("height", std::to_string(result.raster->height)) +
                field("resolution", formatNumber(result.resolution)) +
                field("reference", std::to_string(result.raster->id)) +
                field("name", result.raster->name);
    } else {
        text += field("entities", std::to_string(entitiesAdded)) + field("target", targetLayer);
    }
    text += field("requests", std::to_string(result.stats.requests)) +
            field("cache_hits", std::to_string(result.stats.cacheHits)) +
            field("remote_sources", std::to_string(result.stats.remoteSources)) +
            field("pages", std::to_string(result.stats.pages)) +
            field("duplicates", std::to_string(result.stats.duplicates)) +
            field("cached_result", result.stats.productFromCache ? "yes" : "no") +
            field("retrieved", result.retrieved) + field("licence", result.licence) +
            field("attribution", result.attribution) + field("source", result.sourceUrl) + "\n";
    for (const std::string& warning : result.warnings) {
        text += "warning" + field("text", warning) + "\n";
    }
    return text;
}

std::string formatError(OnlineVerb verb, const katana::core::Error& error)
{
    return "error" + field("verb", toString(verb)) + field("code", std::string(toString(error.code))) +
           field("message", error.message) +
           (error.context.empty() ? std::string() : field("context", error.context)) + "\n";
}

std::string formatCatalogueSearch(const std::vector<CkanResource>& resources)
{
    std::string text = "services count=" + std::to_string(resources.size()) + "\n";
    for (const CkanResource& resource : resources) {
        text += "service" + field("format", resource.format) + field("url", resource.url) +
                field("licence", resource.licence) + field("dataset", resource.dataset) +
                field("title", resource.title) + "\n";
    }
    return text;
}

} // namespace katana::interop
