#include "katana/interop/online_catalogue.hpp"

#include <algorithm>
#include <cmath>
#include <set>
#include <string>

#include <nlohmann/json.hpp>

#include "katana/core/text.hpp"

namespace katana::interop {
namespace {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using Json = nlohmann::json;

// The built-in catalogue, embedded by the compiler (#embed; the directory is
// given by --embed-dir in src/katana_interop/CMakeLists.txt, and the
// dependency file names the JSON, so editing it rebuilds this file). #embed
// is C++26; the pragma keeps a C++23 build from failing -Werror on it.
#pragma GCC diagnostic push
#pragma GCC diagnostic ignored "-Wc++26-extensions"
constexpr unsigned char kEmbeddedCatalogue[] = {
#embed "online_sources.json"
};
#pragma GCC diagnostic pop

struct TypeName {
    OnlineServiceType type;
    const char* name;
};

// The spellings the catalogue uses. One per type: a second spelling would be
// a second way of saying the same thing.
constexpr TypeName kTypeNames[] = {
    {OnlineServiceType::ArcgisExport, "arcgis-export"},
    {OnlineServiceType::ArcgisExportImage, "arcgis-exportimage"},
    {OnlineServiceType::ArcgisQuery, "arcgis-query"},
    {OnlineServiceType::Wms, "wms"},
    {OnlineServiceType::Wmts, "wmts"},
    {OnlineServiceType::Wcs, "wcs"},
    {OnlineServiceType::Wfs, "wfs"},
    {OnlineServiceType::OgcApiFeatures, "oapif"},
    {OnlineServiceType::XyzTiles, "xyz"},
    {OnlineServiceType::Stac, "stac"},
    {OnlineServiceType::Cog, "cog"},
    {OnlineServiceType::Overpass, "overpass"},
    {OnlineServiceType::File, "file"},
    {OnlineServiceType::Ckan, "ckan"},
};

// A value looked up on the layer, then its service, then its provider: the
// inheritance the file format promises.
class Scope {
  public:
    Scope(const Json& provider, const Json& service, const Json& layer)
        : chain_{&layer, &service, &provider}
    {
    }

    [[nodiscard]] const Json* find(const char* key) const
    {
        for (const Json* level : chain_) {
            if (level->is_object()) {
                if (const auto it = level->find(key); it != level->end() && !it->is_null()) {
                    return &*it;
                }
            }
        }
        return nullptr;
    }

    [[nodiscard]] std::string text(const char* key, const std::string& fallback = {}) const
    {
        const Json* value = find(key);
        return value != nullptr && value->is_string() ? value->get<std::string>() : fallback;
    }

    template <typename T>
    [[nodiscard]] T number(const char* key, T fallback) const
    {
        const Json* value = find(key);
        return value != nullptr && value->is_number() ? value->get<T>() : fallback;
    }

  private:
    std::array<const Json*, 3> chain_;
};

std::string where(const std::string& provider, const std::string& layer = {})
{
    return layer.empty() ? provider : provider + "/" + layer;
}

bool isHttpUrl(const std::string& url)
{
    const std::string lower = katana::core::lowered(url.substr(0, 8));
    const std::size_t start = lower.starts_with("https://") ? 8
                              : lower.starts_with("http://") ? 7
                                                             : 0;
    if (start == 0 || url.size() <= start) {
        return false;
    }
    // A host of at least one character before any path, and no whitespace
    // anywhere: a URL a person could paste, not a sentence.
    const std::size_t hostEnd = url.find_first_of("/?#", start);
    if (hostEnd == start) {
        return false;
    }
    return std::none_of(url.begin(), url.end(),
                        [](char c) { return c == ' ' || c == '\t' || c == '\n' || c == '\r'; });
}

bool validLonLat(const LonLatBox& box)
{
    return std::all_of(box.begin(), box.end(), [](double v) { return std::isfinite(v); }) &&
           box[0] >= -180.0 && box[2] <= 180.0 && box[1] >= -90.0 && box[3] <= 90.0 &&
           box[0] < box[2] && box[1] < box[3];
}

bool containsFolded(const std::string& haystack, const std::string& needle)
{
    return katana::core::lowered(haystack).find(needle) != std::string::npos;
}

Json layerToJson(const OnlineLayer& layer)
{
    Json out = Json::object();
    out["id"] = layer.id;
    out["title"] = layer.title;
    out["kind"] = toString(layer.kind);
    if (!layer.layerName.empty()) {
        out["layer"] = layer.layerName;
    }
    if (!layer.idField.empty()) {
        out["idField"] = layer.idField;
    }
    if (!layer.asset.empty()) {
        out["asset"] = layer.asset;
    }
    return out;
}

} // namespace

const char* toString(OnlineServiceType type)
{
    for (const TypeName& entry : kTypeNames) {
        if (entry.type == type) {
            return entry.name;
        }
    }
    return "unknown";
}

const char* toString(OnlineLayerKind kind)
{
    switch (kind) {
    case OnlineLayerKind::Imagery:
        return "imagery";
    case OnlineLayerKind::Elevation:
        return "elevation";
    case OnlineLayerKind::Vector:
        return "vector";
    case OnlineLayerKind::Catalogue:
        return "catalogue";
    }
    return "imagery";
}

std::optional<OnlineServiceType> serviceTypeFromString(std::string_view text)
{
    const std::string lower = katana::core::lowered(text);
    for (const TypeName& entry : kTypeNames) {
        if (lower == entry.name) {
            return entry.type;
        }
    }
    return std::nullopt;
}

std::optional<OnlineLayerKind> layerKindFromString(std::string_view text)
{
    const std::string lower = katana::core::lowered(text);
    for (const OnlineLayerKind kind : {OnlineLayerKind::Imagery, OnlineLayerKind::Elevation,
                                       OnlineLayerKind::Vector, OnlineLayerKind::Catalogue}) {
        if (lower == toString(kind)) {
            return kind;
        }
    }
    return std::nullopt;
}

std::vector<const OnlineLayer*> OnlineProvider::layers() const
{
    std::vector<const OnlineLayer*> out;
    for (const OnlineService& service : services) {
        for (const OnlineLayer& layer : service.layers) {
            out.push_back(&layer);
        }
    }
    return out;
}

const OnlineLayer* OnlineProvider::findLayer(std::string_view layerId) const
{
    const std::string wanted = katana::core::lowered(layerId);
    for (const OnlineLayer* layer : layers()) {
        if (katana::core::lowered(layer->id) == wanted) {
            return layer;
        }
    }
    return nullptr;
}

const OnlineProvider* OnlineCatalogue::findProvider(std::string_view providerId) const
{
    const std::string wanted = katana::core::lowered(providerId);
    for (const OnlineProvider& provider : providers) {
        if (katana::core::lowered(provider.id) == wanted) {
            return &provider;
        }
    }
    return nullptr;
}

std::vector<const OnlineProvider*> OnlineCatalogue::filter(std::string_view text) const
{
    std::vector<std::string> words;
    std::string word;
    for (const char c : katana::core::lowered(text) + " ") {
        if (c == ' ' || c == '\t') {
            if (!word.empty()) {
                words.push_back(word);
            }
            word.clear();
        } else {
            word.push_back(c);
        }
    }
    std::vector<const OnlineProvider*> out;
    for (const OnlineProvider& provider : providers) {
        std::string haystack = provider.id + " " + provider.title + " " + provider.group;
        for (const OnlineLayer* layer : provider.layers()) {
            haystack += " " + layer->id + " " + layer->title + " " + toString(layer->kind);
        }
        if (std::all_of(words.begin(), words.end(),
                        [&](const std::string& needle) { return containsFolded(haystack, needle); })) {
            out.push_back(&provider);
        }
    }
    return out;
}

Result<OnlineCatalogue> parseCatalogue(std::string_view text, bool userDefined)
{
    Json document;
    try {
        document = Json::parse(text.begin(), text.end());
    } catch (const Json::exception& error) {
        return makeError(ErrorCode::ParseFailure, "the online catalogue is not valid JSON",
                         error.what());
    }
    if (!document.is_object() || !document.contains("providers") ||
        !document["providers"].is_array()) {
        return makeError(ErrorCode::ParseFailure,
                         "the online catalogue has no \"providers\" list");
    }
    OnlineCatalogue catalogue;
    const Json empty = Json::object();
    for (const Json& providerJson : document["providers"]) {
        if (!providerJson.is_object()) {
            return makeError(ErrorCode::ParseFailure, "a provider is not an object");
        }
        OnlineProvider provider;
        provider.id = providerJson.value("id", std::string());
        provider.title = providerJson.value("title", provider.id);
        provider.group = providerJson.value("group", std::string("Other"));
        provider.homepage = providerJson.value("homepage", std::string());
        provider.licence = providerJson.value("licence", std::string());
        provider.attribution = providerJson.value("attribution", std::string());
        provider.userDefined = userDefined;
        if (provider.id.empty()) {
            return makeError(ErrorCode::ParseFailure, "a provider has no id",
                             provider.title);
        }
        const Json& services = providerJson.contains("services") ? providerJson["services"] : empty;
        if (!services.is_array()) {
            return makeError(ErrorCode::ParseFailure, "a provider's \"services\" is not a list",
                             provider.id);
        }
        for (const Json& serviceJson : services) {
            if (!serviceJson.is_object()) {
                return makeError(ErrorCode::ParseFailure, "a service is not an object",
                                 provider.id);
            }
            OnlineService service;
            service.id = serviceJson.value("id", std::string());
            service.title = serviceJson.value("title", service.id);
            service.endpoint = serviceJson.value("endpoint", std::string());
            const std::string typeText = serviceJson.value("type", std::string());
            const auto type = serviceTypeFromString(typeText);
            if (!type) {
                return makeError(ErrorCode::ParseFailure, "unknown service type \"" + typeText + "\"",
                                 where(provider.id, service.id));
            }
            service.type = *type;
            const Json& layers = serviceJson.contains("layers") ? serviceJson["layers"] : empty;
            if (!layers.is_array() || layers.empty()) {
                return makeError(ErrorCode::ParseFailure, "a service lists no layers",
                                 where(provider.id, service.id));
            }
            for (const Json& layerJson : layers) {
                if (!layerJson.is_object()) {
                    return makeError(ErrorCode::ParseFailure, "a layer is not an object",
                                     where(provider.id, service.id));
                }
                const Scope scope(providerJson, serviceJson, layerJson);
                OnlineLayer layer;
                layer.providerId = provider.id;
                layer.providerTitle = provider.title;
                layer.group = provider.group;
                layer.serviceId = service.id;
                layer.serviceTitle = service.title;
                layer.id = layerJson.value("id", std::string());
                layer.title = layerJson.value("title", layer.id);
                const std::string kindText = layerJson.value("kind", std::string());
                const auto kind = layerKindFromString(kindText);
                if (!kind) {
                    return makeError(ErrorCode::ParseFailure,
                                     "unknown layer kind \"" + kindText + "\"",
                                     where(provider.id, layer.id));
                }
                layer.kind = *kind;
                layer.type = service.type;
                layer.endpoint = service.endpoint;
                layer.version = scope.text("version");
                layer.layerName = layerJson.value("layer", std::string());
                layer.crs = scope.text("crs");
                if (const Json* coverage = scope.find("coverage")) {
                    if (!coverage->is_array() || coverage->size() != 4) {
                        return makeError(ErrorCode::ParseFailure,
                                         "coverage is not [west, south, east, north]",
                                         where(provider.id, layer.id));
                    }
                    for (std::size_t i = 0; i < 4; ++i) {
                        if (!(*coverage)[i].is_number()) {
                            return makeError(ErrorCode::ParseFailure,
                                             "coverage holds something other than numbers",
                                             where(provider.id, layer.id));
                        }
                        layer.coverage[i] = (*coverage)[i].get<double>();
                    }
                }
                layer.maxRequestPixels = scope.number("maxRequestPixels", layer.maxRequestPixels);
                layer.pageSize = scope.number("pageSize", layer.pageSize);
                layer.maxZoom = scope.number("maxZoom", layer.maxZoom);
                layer.maxTiles = scope.number("maxTiles", layer.maxTiles);
                if (const Json* resolution = scope.find("resolution");
                    resolution != nullptr && resolution->is_number()) {
                    layer.resolution = resolution->get<double>();
                }
                layer.cacheDays = scope.number("cacheDays", layer.cacheDays);
                layer.maxAreaKm2 = scope.number("maxArea", layer.maxAreaKm2);
                layer.timeoutSeconds = scope.number("timeout", layer.timeoutSeconds);
                layer.maxCloud = scope.number("maxCloud", layer.maxCloud);
                layer.days = scope.number("days", layer.days);
                layer.asset = scope.text("asset");
                layer.idField = scope.text("idField");
                layer.time = scope.text("time");
                layer.tiling = scope.text("tiling");
                layer.licence = scope.text("licence");
                layer.attribution = scope.text("attribution");
                layer.homepage = scope.text("homepage");
                if (const Json* key = scope.find("keyRequired"); key != nullptr && key->is_boolean()) {
                    layer.keyRequired = key->get<bool>();
                }
                layer.keyName = scope.text("keyName", provider.id);
                layer.verified = scope.text("verified");
                layer.evidence = scope.text("evidence");
                layer.userDefined = userDefined;
                service.layers.push_back(std::move(layer));
            }
            provider.services.push_back(std::move(service));
        }
        catalogue.providers.push_back(std::move(provider));
    }
    return catalogue;
}

std::string_view builtInCatalogueJson()
{
    return {reinterpret_cast<const char*>(kEmbeddedCatalogue), sizeof kEmbeddedCatalogue};
}

Result<OnlineCatalogue> builtInCatalogue()
{
    return parseCatalogue(builtInCatalogueJson());
}

OnlineCatalogue mergeCatalogues(OnlineCatalogue base, const OnlineCatalogue& overlay)
{
    for (const OnlineProvider& provider : overlay.providers) {
        const std::string wanted = katana::core::lowered(provider.id);
        const auto it = std::find_if(base.providers.begin(), base.providers.end(),
                                     [&](const OnlineProvider& existing) {
                                         return katana::core::lowered(existing.id) == wanted;
                                     });
        if (it != base.providers.end()) {
            *it = provider;
        } else {
            base.providers.push_back(provider);
        }
    }
    return base;
}

std::vector<std::string> validateCatalogue(const OnlineCatalogue& catalogue)
{
    std::vector<std::string> problems;
    std::set<std::string> providerIds;
    for (const OnlineProvider& provider : catalogue.providers) {
        if (!providerIds.insert(katana::core::lowered(provider.id)).second) {
            problems.push_back(provider.id + ": the provider id is used twice");
        }
        if (provider.title.empty()) {
            problems.push_back(provider.id + ": no title");
        }
        if (provider.group.empty()) {
            problems.push_back(provider.id + ": no group");
        }
        std::set<std::string> layerIds;
        for (const OnlineLayer* layer : provider.layers()) {
            const std::string at = where(provider.id, layer->id);
            if (layer->id.empty()) {
                problems.push_back(provider.id + ": a layer has no id");
            } else if (!layerIds.insert(katana::core::lowered(layer->id)).second) {
                problems.push_back(at + ": the layer id is used twice in this provider");
            }
            if (layer->id.find_first_of(" \t/\"") != std::string::npos) {
                problems.push_back(at + ": a layer id may not hold spaces, quotes or slashes");
            }
            if (layer->title.empty()) {
                problems.push_back(at + ": no title");
            }
            if (layer->licence.empty()) {
                problems.push_back(at + ": no licence");
            }
            if (layer->attribution.empty()) {
                problems.push_back(at + ": no attribution to record");
            }
            if (!isHttpUrl(layer->endpoint)) {
                problems.push_back(at + ": the endpoint is not an http or https URL");
            }
            if (!validLonLat(layer->coverage)) {
                problems.push_back(at + ": the coverage is not a valid longitude/latitude box");
            }
            const auto needs = [&](const char* placeholder) {
                if (layer->endpoint.find(placeholder) == std::string::npos) {
                    problems.push_back(at + ": the " + toString(layer->type) +
                                       " endpoint has no " + placeholder);
                }
            };
            switch (layer->type) {
            case OnlineServiceType::XyzTiles:
            case OnlineServiceType::Wmts:
                needs("{z}");
                needs("{x}");
                needs("{y}");
                if (layer->crs != "EPSG:3857") {
                    problems.push_back(at + ": tile templates are on the Web Mercator grid, "
                                            "so the crs must be EPSG:3857");
                }
                if (layer->maxZoom < 0 || layer->maxZoom > 24) {
                    problems.push_back(at + ": maxZoom is outside 0 to 24");
                }
                if (layer->maxTiles < 1 || layer->maxTiles > 4096) {
                    problems.push_back(at + ": maxTiles is outside 1 to 4096");
                }
                break;
            case OnlineServiceType::Cog:
                if (layer->tiling == "degree") {
                    needs("{lat}");
                    needs("{lon}");
                }
                break;
            case OnlineServiceType::ArcgisQuery:
            case OnlineServiceType::Wfs:
            case OnlineServiceType::OgcApiFeatures:
                if (layer->pageSize < 1 || layer->pageSize > 10000) {
                    problems.push_back(at + ": pageSize is outside 1 to 10000");
                }
                if (layer->type != OnlineServiceType::ArcgisQuery || !layer->layerName.empty()) {
                    break;
                }
                problems.push_back(at + ": an ArcGIS query needs the layer id");
                break;
            case OnlineServiceType::ArcgisExport:
            case OnlineServiceType::ArcgisExportImage:
            case OnlineServiceType::Wms:
            case OnlineServiceType::Wcs:
                if (layer->maxRequestPixels < 64 || layer->maxRequestPixels > 16384) {
                    problems.push_back(at + ": maxRequestPixels is outside 64 to 16384");
                }
                if ((layer->type == OnlineServiceType::Wms ||
                     layer->type == OnlineServiceType::Wcs) &&
                    layer->layerName.empty()) {
                    problems.push_back(at + ": a " + std::string(toString(layer->type)) +
                                       " layer needs the service's layer name");
                }
                break;
            case OnlineServiceType::File:
                if (layer->endpoint.find("{layer}") != std::string::npos &&
                    layer->layerName.empty()) {
                    problems.push_back(at + ": the file endpoint names {layer} but the layer "
                                            "gives none");
                }
                break;
            case OnlineServiceType::Stac:
                if (layer->layerName.empty()) {
                    problems.push_back(at + ": a STAC layer needs its collection");
                }
                if (layer->asset.empty()) {
                    problems.push_back(at + ": a STAC layer needs the asset(s) to read");
                }
                break;
            case OnlineServiceType::Overpass:
            case OnlineServiceType::Ckan:
                break;
            }
            const bool imageKind = layer->kind == OnlineLayerKind::Imagery ||
                                   layer->kind == OnlineLayerKind::Elevation;
            const bool imageService = layer->type != OnlineServiceType::ArcgisQuery &&
                                      layer->type != OnlineServiceType::Wfs &&
                                      layer->type != OnlineServiceType::OgcApiFeatures &&
                                      layer->type != OnlineServiceType::Overpass &&
                                      layer->type != OnlineServiceType::File &&
                                      layer->type != OnlineServiceType::Ckan;
            if (imageKind != imageService &&
                layer->kind != OnlineLayerKind::Catalogue) {
                problems.push_back(at + ": a " + std::string(toString(layer->kind)) +
                                   " layer cannot come from a " + toString(layer->type) +
                                   " service");
            }
            if (layer->resolution && !(*layer->resolution > 0.0)) {
                problems.push_back(at + ": the resolution must be positive");
            }
            if (layer->cacheDays < 0) {
                problems.push_back(at + ": cacheDays is negative");
            }
        }
    }
    return problems;
}

std::string providerToJson(const OnlineProvider& provider)
{
    OnlineCatalogue one;
    one.providers.push_back(provider);
    return catalogueToJson(one);
}

std::string catalogueToJson(const OnlineCatalogue& catalogue)
{
    Json document = Json::object();
    document["version"] = 1;
    Json providers = Json::array();
    for (const OnlineProvider& provider : catalogue.providers) {
        Json p = Json::object();
        p["id"] = provider.id;
        p["title"] = provider.title;
        p["group"] = provider.group;
        if (!provider.homepage.empty()) {
            p["homepage"] = provider.homepage;
        }
        p["licence"] = provider.licence;
        p["attribution"] = provider.attribution;
        Json services = Json::array();
        for (const OnlineService& service : provider.services) {
            Json s = Json::object();
            s["id"] = service.id;
            s["title"] = service.title;
            s["type"] = toString(service.type);
            s["endpoint"] = service.endpoint;
            // What every layer of a service shares is written on the
            // service; a discovered service's layers share all of it.
            if (!service.layers.empty()) {
                const OnlineLayer& first = service.layers.front();
                if (!first.version.empty()) {
                    s["version"] = first.version;
                }
                if (!first.crs.empty()) {
                    s["crs"] = first.crs;
                }
                s["coverage"] = first.coverage;
                s["maxRequestPixels"] = first.maxRequestPixels;
                s["pageSize"] = first.pageSize;
                s["maxZoom"] = first.maxZoom;
                s["maxTiles"] = first.maxTiles;
                if (first.licence != provider.licence) {
                    s["licence"] = first.licence;
                }
                if (first.attribution != provider.attribution) {
                    s["attribution"] = first.attribution;
                }
                if (first.keyRequired) {
                    s["keyRequired"] = true;
                    s["keyName"] = first.keyName;
                }
                if (!first.evidence.empty()) {
                    s["evidence"] = first.evidence;
                }
            }
            Json layers = Json::array();
            for (const OnlineLayer& layer : service.layers) {
                Json l = layerToJson(layer);
                if (!service.layers.empty() && layer.coverage != service.layers.front().coverage) {
                    l["coverage"] = layer.coverage;
                }
                if (!layer.crs.empty() && layer.crs != service.layers.front().crs) {
                    l["crs"] = layer.crs;
                }
                layers.push_back(std::move(l));
            }
            s["layers"] = std::move(layers);
            services.push_back(std::move(s));
        }
        p["services"] = std::move(services);
        providers.push_back(std::move(p));
    }
    document["providers"] = std::move(providers);
    return document.dump(1);
}

} // namespace katana::interop
