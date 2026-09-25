#include "katana/interop/online_discovery.hpp"

#include <algorithm>
#include <cmath>
#include <cstdlib>
#include <set>

#include <nlohmann/json.hpp>

#include "katana/core/text.hpp"
#include "katana/gis/reproject.hpp"
#include "katana/gis/web_access.hpp"
#include "katana/interop/online_requests.hpp"

namespace katana::interop {
namespace {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::gis::XmlElement;
using Json = nlohmann::json;

// What a discovered service's layers say about their terms until the person
// checks: the service did not declare a licence Katana can read, and saying
// nothing would record an import as though it had none.
constexpr const char* kUnknownLicence = "not stated by the service - check the publisher's terms";

std::string lower(std::string_view text)
{
    return katana::core::lowered(text);
}

// The URL without its query and fragment: the base a request builder
// appends its own parameters to.
std::string baseOf(const std::string& url)
{
    const std::size_t cut = url.find_first_of("?#");
    return cut == std::string::npos ? url : url.substr(0, cut);
}

// The URL without the OGC request's own parameters (SERVICE, REQUEST,
// VERSION and the like) but with everything else: a MapServer's map=, a
// vendor's own switches, which every request needs. A key parameter stays;
// the user catalogue's writer moves it to the settings.
std::string ogcBaseOf(const std::string& url)
{
    const std::size_t query = url.find('?');
    if (query == std::string::npos) {
        return url;
    }
    static const std::set<std::string> kOwn = {"service", "request", "version", "acceptversions",
                                               "sections", "updatesequence", "acceptformats",
                                               "layers", "typename", "typenames", "coverage",
                                               "format", "outputformat"};
    std::string kept;
    std::size_t at = query + 1;
    const std::size_t end = url.find('#', query);
    const std::string params = url.substr(at, (end == std::string::npos ? url.size() : end) - at);
    std::size_t start = 0;
    while (start <= params.size()) {
        std::size_t stop = params.find('&', start);
        if (stop == std::string::npos) {
            stop = params.size();
        }
        const std::string pair = params.substr(start, stop - start);
        const std::string name = lower(pair.substr(0, pair.find('=')));
        if (!pair.empty() && !kOwn.contains(name)) {
            kept += (kept.empty() ? "" : "&") + pair;
        }
        start = stop + 1;
    }
    return kept.empty() ? url.substr(0, query) : url.substr(0, query) + "?" + kept;
}

std::string hostOf(std::string_view url)
{
    const std::size_t scheme = url.find("://");
    if (scheme == std::string_view::npos) {
        return {};
    }
    const std::size_t start = scheme + 3;
    const std::size_t end = url.find_first_of("/?#:", start);
    return std::string(url.substr(start, end == std::string_view::npos ? url.size() - start
                                                                       : end - start));
}

// An id made of letters, digits and dashes, from any text: what a layer id
// and a provider id may hold (validateCatalogue refuses spaces, quotes and
// slashes, because the verbs split on them).
std::string safeId(std::string_view text, std::size_t limit = 48)
{
    std::string id;
    for (const char c : text) {
        const auto byte = static_cast<unsigned char>(c);
        if ((byte >= 'a' && byte <= 'z') || (byte >= '0' && byte <= '9')) {
            id.push_back(c);
        } else if (byte >= 'A' && byte <= 'Z') {
            id.push_back(static_cast<char>(byte - 'A' + 'a'));
        } else if (!id.empty() && id.back() != '-') {
            id.push_back('-');
        }
        if (id.size() >= limit) {
            break;
        }
    }
    while (!id.empty() && id.back() == '-') {
        id.pop_back();
    }
    return id.empty() ? "layer" : id;
}

// Ids unique within the provider: a second "roads" becomes "roads-2".
std::string uniqueId(std::set<std::string>& used, const std::string& wanted)
{
    std::string id = wanted;
    for (int copy = 2; !used.insert(id).second; ++copy) {
        id = wanted + "-" + std::to_string(copy);
    }
    return id;
}

std::optional<LonLatBox> lonLatFromCorners(const std::string& lowerCorner,
                                           const std::string& upperCorner)
{
    const auto pair = [](const std::string& text) -> std::optional<std::array<double, 2>> {
        const std::string_view view = katana::core::trimmed(text);
        const std::size_t space = view.find_first_of(" \t");
        if (space == std::string_view::npos) {
            return std::nullopt;
        }
        const auto a = katana::core::parseFiniteDouble(katana::core::trimmed(view.substr(0, space)));
        const auto b = katana::core::parseFiniteDouble(katana::core::trimmed(view.substr(space)));
        if (!a || !b) {
            return std::nullopt;
        }
        return std::array<double, 2>{*a, *b};
    };
    const auto low = pair(lowerCorner);
    const auto high = pair(upperCorner);
    if (!low || !high) {
        return std::nullopt;
    }
    return LonLatBox{(*low)[0], (*low)[1], (*high)[0], (*high)[1]};
}

bool sane(const LonLatBox& box)
{
    return box[0] >= -180.0 && box[2] <= 180.0 && box[1] >= -90.0 && box[3] <= 90.0 &&
           box[0] < box[2] && box[1] < box[3];
}

OnlineProvider makeProvider(const std::string& url, const std::string& title)
{
    OnlineProvider provider;
    provider.id = customProviderId(url);
    provider.title = title.empty() ? hostOf(url) : title;
    provider.group = "Custom";
    provider.homepage = baseOf(url);
    provider.licence = kUnknownLicence;
    provider.attribution = provider.title;
    provider.userDefined = true;
    return provider;
}

OnlineLayer baseLayer(const OnlineProvider& provider, const OnlineService& service)
{
    OnlineLayer layer;
    layer.providerId = provider.id;
    layer.providerTitle = provider.title;
    layer.group = provider.group;
    layer.serviceId = service.id;
    layer.serviceTitle = service.title;
    layer.type = service.type;
    layer.endpoint = service.endpoint;
    layer.licence = provider.licence;
    layer.attribution = provider.attribution;
    layer.homepage = provider.homepage;
    layer.keyName = provider.id;
    layer.userDefined = true;
    layer.evidence = "discovered from " + katana::gis::redactUrl(provider.homepage);
    return layer;
}

// The licence a capabilities document declares in its AccessConstraints (or
// Fees), when it says anything but "none": services write "NONE" where they
// mean "not stated", and that is not a licence.
std::string declaredLicence(const XmlElement& root)
{
    const XmlElement* identification = root.child("ServiceIdentification");
    const XmlElement* service = identification != nullptr ? identification : root.child("Service");
    if (service == nullptr) {
        return {};
    }
    for (const char* field : {"AccessConstraints", "Fees"}) {
        const std::string text = service->childText(field);
        const std::string folded = lower(text);
        if (!text.empty() && folded != "none" && folded != "no" && folded != "unknown") {
            return text.size() > 200 ? text.substr(0, 200) + "..." : text;
        }
    }
    return {};
}

std::string serviceTitle(const XmlElement& root)
{
    for (const char* section : {"ServiceIdentification", "Service"}) {
        if (const XmlElement* service = root.child(section)) {
            const std::string title = service->childText("Title");
            if (!title.empty()) {
                return title;
            }
        }
    }
    return {};
}

// Among the CRSs a layer offers, the one Katana asks in: Web Mercator where
// offered (every web service draws it, and it needs no axis swap), then
// WGS 84, then whatever the service lists first.
std::string preferredCrs(const std::vector<std::string>& offered)
{
    for (const char* wanted : {"EPSG:3857", "EPSG:900913", "EPSG:4326", "CRS:84"}) {
        for (const std::string& crs : offered) {
            if (lower(crs) == lower(wanted)) {
                return crs == "EPSG:900913" ? "EPSG:3857" : crs;
            }
        }
    }
    return offered.empty() ? std::string("EPSG:4326") : offered.front();
}

// Named layers of a WMS, depth first, each inheriting its parents' CRS list
// and bounding box as the WMS specification says (7.2.4.8).
void collectWmsLayers(const XmlElement& element, std::vector<std::string> crs,
                      std::optional<LonLatBox> box, const OnlineProvider& provider,
                      OnlineService& service, std::set<std::string>& used, bool v13)
{
    for (const XmlElement* code : element.childrenNamed(v13 ? "CRS" : "SRS")) {
        // 1.1.1 may list several codes in one element, space separated.
        std::string text = code->text;
        for (std::size_t start = 0; start < text.size();) {
            std::size_t end = text.find(' ', start);
            if (end == std::string::npos) {
                end = text.size();
            }
            if (end > start) {
                crs.push_back(text.substr(start, end - start));
            }
            start = end + 1;
        }
    }
    if (const XmlElement* bounds = element.child("EX_GeographicBoundingBox")) {
        const auto w = katana::core::parseFiniteDouble(bounds->childText("westBoundLongitude"));
        const auto s = katana::core::parseFiniteDouble(bounds->childText("southBoundLatitude"));
        const auto e = katana::core::parseFiniteDouble(bounds->childText("eastBoundLongitude"));
        const auto n = katana::core::parseFiniteDouble(bounds->childText("northBoundLatitude"));
        if (w && s && e && n) {
            box = LonLatBox{*w, *s, *e, *n};
        }
    } else if (const XmlElement* latLon = element.child("LatLonBoundingBox")) {
        const auto w = katana::core::parseFiniteDouble(latLon->attribute("minx"));
        const auto s = katana::core::parseFiniteDouble(latLon->attribute("miny"));
        const auto e = katana::core::parseFiniteDouble(latLon->attribute("maxx"));
        const auto n = katana::core::parseFiniteDouble(latLon->attribute("maxy"));
        if (w && s && e && n) {
            box = LonLatBox{*w, *s, *e, *n};
        }
    }
    const std::string name = element.childText("Name");
    if (!name.empty()) {
        OnlineLayer layer = baseLayer(provider, service);
        layer.id = uniqueId(used, safeId(name));
        layer.title = element.childText("Title").empty() ? name : element.childText("Title");
        layer.kind = OnlineLayerKind::Imagery;
        layer.layerName = name;
        layer.crs = preferredCrs(crs);
        if (box && sane(*box)) {
            layer.coverage = *box;
        }
        service.layers.push_back(std::move(layer));
    }
    for (const XmlElement* child : element.childrenNamed("Layer")) {
        collectWmsLayers(*child, crs, box, provider, service, used, v13);
    }
}

Result<Json> parseJson(std::string_view text)
{
    try {
        return Json::parse(text.begin(), text.end());
    } catch (const Json::exception& error) {
        return makeError(ErrorCode::ParseFailure, "the service's description is not JSON",
                         error.what());
    }
}

// An ArcGIS extent ({xmin, ymin, xmax, ymax, spatialReference}) as a
// longitude/latitude box, when the extent's CRS can be read.
std::optional<LonLatBox> arcgisExtent(const Json& extent)
{
    if (!extent.is_object() || !extent.contains("xmin") || !extent["xmin"].is_number()) {
        return std::nullopt;
    }
    std::string crs;
    if (extent.contains("spatialReference") && extent["spatialReference"].is_object()) {
        const Json& reference = extent["spatialReference"];
        const int wkid = reference.contains("latestWkid") && reference["latestWkid"].is_number()
                             ? reference["latestWkid"].get<int>()
                         : reference.contains("wkid") && reference["wkid"].is_number()
                             ? reference["wkid"].get<int>()
                             : 0;
        if (wkid == 102100 || wkid == 102113) {
            crs = "EPSG:3857"; // Esri's old codes for Web Mercator
        } else if (wkid > 0) {
            crs = "EPSG:" + std::to_string(wkid);
        } else if (reference.contains("wkt") && reference["wkt"].is_string()) {
            crs = reference["wkt"].get<std::string>();
        }
    }
    if (crs.empty()) {
        return std::nullopt;
    }
    try {
        katana::gis::CrsBox native;
        native.minX = extent["xmin"].get<double>();
        native.minY = extent["ymin"].get<double>();
        native.maxX = extent["xmax"].get<double>();
        native.maxY = extent["ymax"].get<double>();
        const auto box = katana::gis::transformBox(native, crs, "EPSG:4326");
        if (!box) {
            return std::nullopt;
        }
        LonLatBox out{std::max(-180.0, box->minX), std::max(-90.0, box->minY),
                      std::min(180.0, box->maxX), std::min(90.0, box->maxY)};
        return sane(out) ? std::optional<LonLatBox>(out) : std::nullopt;
    } catch (const Json::exception&) {
        return std::nullopt;
    }
}

} // namespace

std::optional<OnlineServiceType> guessServiceType(std::string_view url)
{
    const std::string folded = lower(url);
    const std::string path = lower(baseOf(std::string(url)));
    if (folded.find("/imageserver") != std::string::npos) {
        return OnlineServiceType::ArcgisExportImage;
    }
    if (folded.find("/featureserver") != std::string::npos) {
        return OnlineServiceType::ArcgisQuery;
    }
    if (folded.find("/mapserver") != std::string::npos &&
        folded.find("wmsserver") == std::string::npos &&
        folded.find("wfsserver") == std::string::npos &&
        folded.find("wcsserver") == std::string::npos &&
        folded.find("/wmts") == std::string::npos) {
        return OnlineServiceType::ArcgisExport;
    }
    if (folded.find("service=wmts") != std::string::npos ||
        folded.find("wmtscapabilities") != std::string::npos || folded.find("/wmts") != std::string::npos) {
        return OnlineServiceType::Wmts;
    }
    if (folded.find("service=wfs") != std::string::npos || folded.find("wfsserver") != std::string::npos) {
        return OnlineServiceType::Wfs;
    }
    if (folded.find("service=wcs") != std::string::npos || folded.find("wcsserver") != std::string::npos) {
        return OnlineServiceType::Wcs;
    }
    if (folded.find("service=wms") != std::string::npos || folded.find("wmsserver") != std::string::npos) {
        return OnlineServiceType::Wms;
    }
    if (path.ends_with(".tif") || path.ends_with(".tiff")) {
        return OnlineServiceType::Cog;
    }
    if (folded.find("stac") != std::string::npos || path.ends_with(".json")) {
        return OnlineServiceType::Stac;
    }
    if (path.ends_with("/collections") || folded.find("/ogc/features") != std::string::npos) {
        return OnlineServiceType::OgcApiFeatures;
    }
    return std::nullopt;
}

std::string descriptionUrl(OnlineServiceType type, const std::string& url)
{
    const std::string base = baseOf(url);
    const auto trimmedBase = [&] {
        std::string out = base;
        while (!out.empty() && out.back() == '/') {
            out.pop_back();
        }
        return out;
    };
    switch (type) {
    case OnlineServiceType::Wms:
        return withQuery(ogcBaseOf(url), "SERVICE=WMS&REQUEST=GetCapabilities");
    case OnlineServiceType::Wmts:
        if (lower(base).ends_with(".xml")) {
            return base;
        }
        return withQuery(ogcBaseOf(url), "SERVICE=WMTS&REQUEST=GetCapabilities&VERSION=1.0.0");
    case OnlineServiceType::Wfs:
        return withQuery(ogcBaseOf(url), "SERVICE=WFS&REQUEST=GetCapabilities");
    case OnlineServiceType::Wcs:
        return withQuery(ogcBaseOf(url), "SERVICE=WCS&REQUEST=GetCapabilities&VERSION=1.0.0");
    case OnlineServiceType::ArcgisExport:
    case OnlineServiceType::ArcgisExportImage:
    case OnlineServiceType::ArcgisQuery:
        return withQuery(base, "f=json");
    case OnlineServiceType::OgcApiFeatures: {
        const std::string root = trimmedBase();
        return withQuery(lower(root).ends_with("/collections") ? root : root + "/collections",
                         "f=json");
    }
    case OnlineServiceType::Stac: {
        const std::string root = trimmedBase();
        if (lower(root).ends_with(".json") || lower(root).ends_with("/collections")) {
            return url;
        }
        return root + "/collections";
    }
    default:
        return url;
    }
}

std::string customProviderId(std::string_view url)
{
    // The host says whose it is; eight hex digits of the address without its
    // query say which service, so two services of one server are two
    // providers - one host alone let the second replace the first.
    return "custom-" + safeId(hostOf(url), 40) + "-" +
           katana::gis::sha256Hex(baseOf(std::string(url))).substr(0, 8);
}

Result<OnlineProvider> parseWmsCapabilities(std::string_view xml, const std::string& url)
{
    auto root = katana::gis::parseXml(xml);
    if (!root) {
        return root.error();
    }
    const std::string_view name = root->localName();
    if (name != "WMS_Capabilities" && name != "WMT_MS_Capabilities") {
        return makeError(ErrorCode::ParseFailure, "the answer is not WMS capabilities",
                         std::string(name));
    }
    const std::string version = root->attribute("version").empty() ? "1.3.0" : root->attribute("version");
    OnlineProvider provider = makeProvider(url, serviceTitle(*root));
    if (const std::string licence = declaredLicence(*root); !licence.empty()) {
        provider.licence = licence;
    }
    OnlineService service;
    service.id = "wms";
    service.title = provider.title + " (WMS)";
    service.type = OnlineServiceType::Wms;
    service.endpoint = ogcBaseOf(url);
    // The GetMap address the service names, which may differ from where its
    // capabilities were found.
    if (const XmlElement* capability = root->child("Capability")) {
        if (const XmlElement* request = capability->child("Request")) {
            if (const XmlElement* getMap = request->child("GetMap")) {
                for (const XmlElement* dcp : getMap->childrenNamed("DCPType")) {
                    if (const XmlElement* http = dcp->child("HTTP")) {
                        if (const XmlElement* get = http->child("Get")) {
                            if (const XmlElement* resource = get->child("OnlineResource")) {
                                const std::string href = resource->attribute("href");
                                if (href.starts_with("http")) {
                                    service.endpoint = ogcBaseOf(href);
                                }
                            }
                        }
                    }
                }
            }
        }
        std::set<std::string> used;
        for (const XmlElement* layer : capability->childrenNamed("Layer")) {
            collectWmsLayers(*layer, {}, std::nullopt, provider, service, used,
                             version.starts_with("1.3"));
        }
    }
    for (OnlineLayer& layer : service.layers) {
        layer.endpoint = service.endpoint;
        layer.version = version;
        layer.licence = provider.licence;
    }
    if (service.layers.empty()) {
        return makeError(ErrorCode::NotFound, "the WMS lists no named layer");
    }
    provider.services.push_back(std::move(service));
    return provider;
}

Result<OnlineProvider> parseWmtsCapabilities(std::string_view xml, const std::string& url)
{
    auto root = katana::gis::parseXml(xml);
    if (!root) {
        return root.error();
    }
    if (root->localName() != "Capabilities") {
        return makeError(ErrorCode::ParseFailure, "the answer is not WMTS capabilities",
                         std::string(root->localName()));
    }
    OnlineProvider provider = makeProvider(url, serviceTitle(*root));
    if (const std::string licence = declaredLicence(*root); !licence.empty()) {
        provider.licence = licence;
    }
    const XmlElement* contents = root->child("Contents");
    if (contents == nullptr) {
        return makeError(ErrorCode::ParseFailure, "the WMTS capabilities have no Contents");
    }
    // Only matrix sets on the Web Mercator grid, level for level: those are
    // what the tile path computes with (webMercatorTileMatrixSet). Each is
    // recorded with the prefix its matrix ids carry ("EPSG:3857:" or "") and
    // its deepest level.
    struct GridInfo {
        std::string prefix;
        int maxZoom = 0;
    };
    std::map<std::string, GridInfo> grids;
    const TileMatrixSet reference = webMercatorTileMatrixSet(24);
    for (const XmlElement* set : contents->childrenNamed("TileMatrixSet")) {
        const std::string id = set->childText("Identifier");
        const std::string crs = lower(set->childText("SupportedCRS"));
        if (crs.find("3857") == std::string::npos && crs.find("900913") == std::string::npos) {
            continue;
        }
        const std::vector<const XmlElement*> matrices = set->childrenNamed("TileMatrix");
        bool matches = !matrices.empty();
        std::string prefix;
        int level = 0;
        for (const XmlElement* matrix : matrices) {
            const auto scale = katana::core::parseFiniteDouble(matrix->childText("ScaleDenominator"));
            const std::string matrixId = matrix->childText("Identifier");
            const std::size_t colon = matrixId.rfind(':');
            const std::string levelText = colon == std::string::npos ? matrixId : matrixId.substr(colon + 1);
            const std::string thisPrefix = colon == std::string::npos ? "" : matrixId.substr(0, colon + 1);
            const auto parsed = katana::core::parseFiniteDouble(levelText);
            if (!scale || !parsed || level >= static_cast<int>(reference.matrices.size())) {
                matches = false;
                break;
            }
            const double expected = reference.matrices[static_cast<std::size_t>(level)].scaleDenominator;
            if (std::abs(*scale - expected) > expected * 1e-6 ||
                static_cast<int>(*parsed) != level || (level > 0 && thisPrefix != prefix)) {
                matches = false;
                break;
            }
            prefix = thisPrefix;
            ++level;
        }
        if (matches) {
            grids[id] = GridInfo{prefix, level - 1};
        }
    }
    OnlineService service;
    service.id = "wmts";
    service.title = provider.title + " (WMTS)";
    service.type = OnlineServiceType::Wmts;
    service.endpoint = ogcBaseOf(url);
    std::set<std::string> used;
    for (const XmlElement* layerElement : contents->childrenNamed("Layer")) {
        const std::string identifier = layerElement->childText("Identifier");
        std::string gridId;
        for (const XmlElement* link : layerElement->childrenNamed("TileMatrixSetLink")) {
            if (grids.contains(link->childText("TileMatrixSet"))) {
                gridId = link->childText("TileMatrixSet");
                break;
            }
        }
        if (identifier.empty() || gridId.empty()) {
            continue;
        }
        const GridInfo& grid = grids[gridId];
        std::string style = "default";
        for (const XmlElement* styleElement : layerElement->childrenNamed("Style")) {
            if (styleElement->attribute("isDefault") == "true" || style == "default") {
                const std::string id = styleElement->childText("Identifier");
                if (!id.empty()) {
                    style = id;
                }
            }
        }
        std::string format = layerElement->childText("Format");
        std::string pattern;
        for (const XmlElement* resource : layerElement->childrenNamed("ResourceURL")) {
            if (resource->attribute("resourceType") == "tile") {
                pattern = resource->attribute("template");
                format = resource->attribute("format").empty() ? format : resource->attribute("format");
                break;
            }
        }
        if (pattern.empty()) {
            // KVP only: the same request as a template.
            pattern = withQuery(service.endpoint,
                                "SERVICE=WMTS&REQUEST=GetTile&VERSION=1.0.0&LAYER=" +
                                    katana::gis::percentEncode(identifier) + "&STYLE=" +
                                    katana::gis::percentEncode(style) + "&FORMAT=" +
                                    katana::gis::percentEncode(format.empty() ? "image/png" : format) +
                                    "&TILEMATRIXSET=" + katana::gis::percentEncode(gridId) +
                                    "&TILEMATRIX={TileMatrix}&TILEROW={TileRow}&TILECOL={TileCol}");
        }
        // One template form for the tile path: {z} {x} {y}, the matrix id's
        // prefix written in, the style, layer and set fixed, and a time
        // dimension left as {time} with its default.
        const auto replace = [](std::string text, const std::string& from, const std::string& to) {
            for (std::size_t at = text.find(from); at != std::string::npos; at = text.find(from, at + to.size())) {
                text.replace(at, from.size(), to);
            }
            return text;
        };
        pattern = replace(pattern, "{TileMatrixSet}", gridId);
        pattern = replace(pattern, "{TileMatrix}", grid.prefix + "{z}");
        pattern = replace(pattern, "{TileCol}", "{x}");
        pattern = replace(pattern, "{TileRow}", "{y}");
        pattern = replace(pattern, "{Style}", style);
        pattern = replace(pattern, "{style}", style);
        pattern = replace(pattern, "{Layer}", identifier);
        pattern = replace(pattern, "{Time}", "{time}");
        OnlineLayer layer = baseLayer(provider, service);
        layer.id = uniqueId(used, safeId(identifier));
        layer.title = layerElement->childText("Title").empty() ? identifier : layerElement->childText("Title");
        layer.kind = OnlineLayerKind::Imagery;
        layer.layerName = identifier;
        layer.endpoint = pattern;
        layer.crs = "EPSG:3857";
        layer.maxZoom = grid.maxZoom;
        layer.maxTiles = 256;
        layer.time = "default";
        if (const XmlElement* bounds = layerElement->child("WGS84BoundingBox")) {
            if (const auto box = lonLatFromCorners(bounds->childText("LowerCorner"),
                                                   bounds->childText("UpperCorner"));
                box && sane(*box)) {
                layer.coverage = *box;
            }
        }
        service.layers.push_back(std::move(layer));
    }
    if (service.layers.empty()) {
        return makeError(ErrorCode::Unsupported,
                         "the WMTS offers no layer on the Web Mercator (GoogleMapsCompatible) "
                         "grid, which is the tile grid Katana reads");
    }
    provider.services.push_back(std::move(service));
    return provider;
}

Result<OnlineProvider> parseWfsCapabilities(std::string_view xml, const std::string& url)
{
    auto root = katana::gis::parseXml(xml);
    if (!root) {
        return root.error();
    }
    if (root->localName() != "WFS_Capabilities") {
        return makeError(ErrorCode::ParseFailure, "the answer is not WFS capabilities",
                         std::string(root->localName()));
    }
    const std::string version = root->attribute("version").empty() ? "2.0.0" : root->attribute("version");
    OnlineProvider provider = makeProvider(url, serviceTitle(*root));
    if (const std::string licence = declaredLicence(*root); !licence.empty()) {
        provider.licence = licence;
    }
    OnlineService service;
    service.id = "wfs";
    service.title = provider.title + " (WFS)";
    service.type = OnlineServiceType::Wfs;
    service.endpoint = ogcBaseOf(url);
    std::set<std::string> used;
    if (const XmlElement* list = root->child("FeatureTypeList")) {
        for (const XmlElement* type : list->childrenNamed("FeatureType")) {
            const std::string name = type->childText("Name");
            if (name.empty()) {
                continue;
            }
            OnlineLayer layer = baseLayer(provider, service);
            layer.id = uniqueId(used, safeId(name));
            layer.title = type->childText("Title").empty() ? name : type->childText("Title");
            layer.kind = OnlineLayerKind::Vector;
            layer.layerName = name;
            layer.version = version;
            std::string crs = type->childText("DefaultCRS");
            if (crs.empty()) {
                crs = type->childText("DefaultSRS");
            }
            // urn:ogc:def:crs:EPSG::28356 -> EPSG:28356, the form the request
            // builders and GDAL's user input both take.
            if (const std::size_t at = crs.find("EPSG::"); at != std::string::npos) {
                crs = "EPSG:" + crs.substr(at + 6);
            }
            layer.crs = crs.empty() ? "EPSG:4326" : crs;
            if (const XmlElement* bounds = type->child("WGS84BoundingBox")) {
                if (const auto box = lonLatFromCorners(bounds->childText("LowerCorner"),
                                                       bounds->childText("UpperCorner"));
                    box && sane(*box)) {
                    layer.coverage = *box;
                }
            }
            service.layers.push_back(std::move(layer));
        }
    }
    if (service.layers.empty()) {
        return makeError(ErrorCode::NotFound, "the WFS lists no feature type");
    }
    provider.services.push_back(std::move(service));
    return provider;
}

Result<OnlineProvider> parseWcsCapabilities(std::string_view xml, const std::string& url)
{
    auto root = katana::gis::parseXml(xml);
    if (!root) {
        return root.error();
    }
    if (root->localName() != "WCS_Capabilities" && root->localName() != "Capabilities") {
        return makeError(ErrorCode::ParseFailure, "the answer is not WCS capabilities",
                         std::string(root->localName()));
    }
    std::string title = serviceTitle(*root);
    if (title.empty()) {
        if (const XmlElement* service = root->child("Service")) {
            title = service->childText("label");
        }
    }
    OnlineProvider provider = makeProvider(url, title);
    OnlineService service;
    service.id = "wcs";
    service.title = provider.title + " (WCS)";
    service.type = OnlineServiceType::Wcs;
    service.endpoint = ogcBaseOf(url);
    std::set<std::string> used;
    if (const XmlElement* metadata = root->child("ContentMetadata")) {
        for (const XmlElement* brief : metadata->childrenNamed("CoverageOfferingBrief")) {
            const std::string name = brief->childText("name");
            if (name.empty()) {
                continue;
            }
            OnlineLayer layer = baseLayer(provider, service);
            layer.id = uniqueId(used, safeId(name));
            layer.title = brief->childText("label").empty() ? name : brief->childText("label");
            layer.kind = OnlineLayerKind::Elevation;
            layer.layerName = name;
            layer.version = "1.0.0";
            layer.crs = "EPSG:4326";
            if (const XmlElement* envelope = brief->child("lonLatEnvelope")) {
                const auto positions = envelope->childrenNamed("pos");
                if (positions.size() == 2) {
                    if (const auto box = lonLatFromCorners(positions[0]->text, positions[1]->text);
                        box && sane(*box)) {
                        layer.coverage = *box;
                    }
                }
            }
            service.layers.push_back(std::move(layer));
        }
    }
    if (service.layers.empty()) {
        return makeError(ErrorCode::NotFound, "the WCS lists no coverage (only WCS 1.0.0 is read)");
    }
    provider.services.push_back(std::move(service));
    return provider;
}

Result<OnlineProvider> parseArcgisDescription(std::string_view text, const std::string& url)
{
    auto document = parseJson(text);
    if (!document) {
        return document.error();
    }
    const Json& json = *document;
    if (json.is_object() && json.contains("error")) {
        return makeError(ErrorCode::FileImportFailure, "the ArcGIS service refused the request",
                         json["error"].dump());
    }
    if (!json.is_object()) {
        return makeError(ErrorCode::ParseFailure, "the ArcGIS description is not an object");
    }
    const std::string base = baseOf(url);
    const std::string folded = lower(base);
    const std::string title = json.value("mapName", json.value("name", json.value("serviceDescription", hostOf(url))));
    OnlineProvider provider = makeProvider(url, title.empty() ? hostOf(url) : title.substr(0, 120));
    if (json.contains("copyrightText") && json["copyrightText"].is_string() &&
        !json["copyrightText"].get<std::string>().empty()) {
        provider.attribution = json["copyrightText"].get<std::string>();
    }
    const int pageSize = json.contains("maxRecordCount") && json["maxRecordCount"].is_number()
                             ? std::clamp(json["maxRecordCount"].get<int>(), 1, 10000)
                             : 1000;
    const std::optional<LonLatBox> extent =
        json.contains("fullExtent") ? arcgisExtent(json["fullExtent"])
        : json.contains("extent")   ? arcgisExtent(json["extent"])
                                    : std::nullopt;
    const auto applyExtent = [&](OnlineLayer& layer) {
        if (extent) {
            layer.coverage = *extent;
        }
    };
    std::set<std::string> used;

    // One layer of a MapServer or FeatureServer: .../MapServer/9
    const std::size_t lastSlash = base.find_last_of('/');
    const std::string tail = lastSlash == std::string::npos ? "" : base.substr(lastSlash + 1);
    const bool layerUrl = !tail.empty() && std::all_of(tail.begin(), tail.end(), [](char c) {
        return c >= '0' && c <= '9';
    });
    if (layerUrl) {
        OnlineService service;
        service.id = "query";
        service.title = provider.title;
        service.type = OnlineServiceType::ArcgisQuery;
        service.endpoint = base.substr(0, lastSlash);
        OnlineLayer layer = baseLayer(provider, service);
        layer.id = uniqueId(used, safeId(json.value("name", tail)));
        layer.title = json.value("name", tail);
        layer.kind = OnlineLayerKind::Vector;
        layer.layerName = tail;
        layer.pageSize = pageSize;
        // The layer's own id field (OBJECTID, FID, OID...), which pages are
        // ordered and joined by.
        if (json.contains("objectIdField") && json["objectIdField"].is_string()) {
            layer.idField = json["objectIdField"].get<std::string>();
        } else if (json.contains("fields") && json["fields"].is_array()) {
            for (const Json& field : json["fields"]) {
                if (field.is_object() && field.value("type", std::string()) == "esriFieldTypeOID") {
                    layer.idField = field.value("name", std::string());
                    break;
                }
            }
        }
        applyExtent(layer);
        service.layers.push_back(std::move(layer));
        provider.services.push_back(std::move(service));
        return provider;
    }

    if (folded.ends_with("/imageserver")) {
        OnlineService service;
        service.id = "image";
        service.title = provider.title;
        service.type = OnlineServiceType::ArcgisExportImage;
        service.endpoint = base;
        OnlineLayer layer = baseLayer(provider, service);
        const std::string pixelType = json.value("pixelType", std::string());
        const int bands = json.contains("bandCount") && json["bandCount"].is_number()
                              ? json["bandCount"].get<int>()
                              : 1;
        // One band of anything but unsigned bytes is values, not a picture:
        // an elevation model, read as numbers.
        const bool values = bands == 1 && pixelType != "U8";
        layer.id = uniqueId(used, safeId(json.value("name", std::string(values ? "elevation" : "image"))));
        layer.title = json.value("name", provider.title);
        layer.kind = values ? OnlineLayerKind::Elevation : OnlineLayerKind::Imagery;
        layer.crs = "EPSG:3857";
        layer.maxRequestPixels = json.contains("maxImageWidth") && json["maxImageWidth"].is_number()
                                     ? std::clamp(json["maxImageWidth"].get<int>(), 64, 16384)
                                     : 4000;
        if (json.contains("pixelSizeX") && json["pixelSizeX"].is_number()) {
            const double size = json["pixelSizeX"].get<double>();
            // Degrees when the service is geographic; metres otherwise.
            layer.resolution = size < 0.01 ? size * 111320.0 : size;
        }
        applyExtent(layer);
        service.layers.push_back(std::move(layer));
        provider.services.push_back(std::move(service));
        return provider;
    }

    // A MapServer or FeatureServer: the map as a whole, as an image (a
    // MapServer only), and each feature layer as vectors.
    const bool mapServer = folded.ends_with("/mapserver");
    if (mapServer) {
        OnlineService service;
        service.id = "map";
        service.title = provider.title + " (map image)";
        service.type = OnlineServiceType::ArcgisExport;
        service.endpoint = base;
        OnlineLayer layer = baseLayer(provider, service);
        layer.id = uniqueId(used, "map");
        layer.title = provider.title + " - the whole map as an image";
        layer.kind = OnlineLayerKind::Imagery;
        layer.crs = "EPSG:3857";
        layer.maxRequestPixels = json.contains("maxImageWidth") && json["maxImageWidth"].is_number()
                                     ? std::clamp(json["maxImageWidth"].get<int>(), 64, 16384)
                                     : 2048;
        applyExtent(layer);
        service.layers.push_back(std::move(layer));
        provider.services.push_back(std::move(service));
    }
    OnlineService features;
    features.id = "features";
    features.title = provider.title + " (features)";
    features.type = OnlineServiceType::ArcgisQuery;
    features.endpoint = base;
    if (json.contains("layers") && json["layers"].is_array()) {
        for (const Json& entry : json["layers"]) {
            if (!entry.is_object() || !entry.contains("id") || !entry["id"].is_number()) {
                continue;
            }
            // A group layer has sublayers and no features of its own.
            if (entry.contains("subLayerIds") && entry["subLayerIds"].is_array() &&
                !entry["subLayerIds"].empty()) {
                continue;
            }
            const std::string type = entry.value("type", std::string("Feature Layer"));
            if (type != "Feature Layer" && type != "Table") {
                continue;
            }
            if (type == "Table") {
                continue; // no geometry to draw
            }
            const std::string name = entry.value("name", std::to_string(entry["id"].get<int>()));
            OnlineLayer layer = baseLayer(provider, features);
            layer.id = uniqueId(used, safeId(name));
            layer.title = name;
            layer.kind = OnlineLayerKind::Vector;
            layer.layerName = std::to_string(entry["id"].get<int>());
            layer.pageSize = pageSize;
            applyExtent(layer);
            features.layers.push_back(std::move(layer));
        }
    }
    if (!features.layers.empty()) {
        provider.services.push_back(std::move(features));
    }
    if (provider.services.empty()) {
        return makeError(ErrorCode::NotFound, "the ArcGIS service offers no layer Katana can import");
    }
    return provider;
}

Result<OnlineProvider> parseOapifCollections(std::string_view text, const std::string& url)
{
    auto document = parseJson(text);
    if (!document) {
        return document.error();
    }
    const Json& json = *document;
    if (!json.is_object() || !json.contains("collections") || !json["collections"].is_array()) {
        return makeError(ErrorCode::ParseFailure, "the answer lists no collections");
    }
    std::string root = baseOf(url);
    while (!root.empty() && root.back() == '/') {
        root.pop_back();
    }
    if (lower(root).ends_with("/collections")) {
        root.resize(root.size() - std::string("/collections").size());
    }
    OnlineProvider provider = makeProvider(url, json.value("title", hostOf(url)));
    OnlineService service;
    service.id = "features";
    service.title = provider.title + " (OGC API - Features)";
    service.type = OnlineServiceType::OgcApiFeatures;
    service.endpoint = root;
    std::set<std::string> used;
    for (const Json& collection : json["collections"]) {
        if (!collection.is_object() || !collection.contains("id")) {
            continue;
        }
        const std::string id = collection["id"].is_string() ? collection["id"].get<std::string>() : collection["id"].dump();
        OnlineLayer layer = baseLayer(provider, service);
        layer.id = uniqueId(used, safeId(id));
        layer.title = collection.value("title", id);
        layer.kind = OnlineLayerKind::Vector;
        layer.layerName = id;
        layer.crs = "OGC:CRS84";
        if (collection.contains("extent") && collection["extent"].contains("spatial") &&
            collection["extent"]["spatial"].contains("bbox") &&
            collection["extent"]["spatial"]["bbox"].is_array() &&
            !collection["extent"]["spatial"]["bbox"].empty()) {
            const Json& box = collection["extent"]["spatial"]["bbox"][0];
            if (box.is_array() && box.size() >= 4 && box[0].is_number()) {
                const LonLatBox coverage{box[0].get<double>(), box[1].get<double>(),
                                         box[box.size() >= 6 ? 3 : 2].get<double>(),
                                         box[box.size() >= 6 ? 4 : 3].get<double>()};
                if (sane(coverage)) {
                    layer.coverage = coverage;
                }
            }
        }
        // A STAC collection listed at /collections of a STAC API is not a
        // feature collection; parseStacDescription reads those.
        if (collection.value("type", std::string()) == "Collection" && collection.contains("stac_version")) {
            continue;
        }
        service.layers.push_back(std::move(layer));
    }
    if (service.layers.empty()) {
        return makeError(ErrorCode::NotFound, "the OGC API lists no feature collection");
    }
    provider.services.push_back(std::move(service));
    return provider;
}

Result<OnlineProvider> parseStacDescription(std::string_view text, const std::string& url)
{
    auto document = parseJson(text);
    if (!document) {
        return document.error();
    }
    const Json& json = *document;
    // One item: a layer reading that item's assets.
    if (json.is_object() && json.value("type", std::string()) == "Feature" && json.contains("assets")) {
        auto items = parseStacItems(text);
        if (!items || items->empty()) {
            return makeError(ErrorCode::ParseFailure, "the STAC item could not be read");
        }
        const StacItem& item = items->front();
        OnlineProvider provider = makeProvider(url, "STAC item " + item.id);
        OnlineService service;
        service.id = "item";
        service.title = provider.title;
        service.type = OnlineServiceType::Stac;
        service.endpoint = baseOf(url);
        OnlineLayer layer = baseLayer(provider, service);
        layer.id = safeId(item.id);
        layer.title = item.id + (item.datetime.empty() ? "" : " (" + item.datetime.substr(0, 10) + ")");
        layer.kind = OnlineLayerKind::Imagery;
        layer.layerName = json.value("collection", std::string("item"));
        layer.asset = item.assets.contains("visual")                                     ? "visual"
                      : item.assets.contains("red") && item.assets.contains("green") &&
                              item.assets.contains("blue")
                          ? "red,green,blue"
                          : std::string();
        if (layer.asset.empty()) {
            return makeError(ErrorCode::Unsupported,
                             "the STAC item has no visual asset and no red, green and blue bands");
        }
        layer.coverage = {item.bounds.minX, item.bounds.minY, item.bounds.maxX, item.bounds.maxY};
        layer.resolution = 10.0;
        layer.days = 0;
        if (layer.layerName.starts_with("sentinel-2")) {
            layer.licence = "Copernicus Sentinel data terms (free, full and open)";
            layer.attribution = "Contains modified Copernicus Sentinel data";
        }
        service.layers.push_back(std::move(layer));
        provider.services.push_back(std::move(service));
        return provider;
    }
    // A STAC API's collections: each a layer searched through <root>/search.
    if (!json.is_object() || !json.contains("collections") || !json["collections"].is_array()) {
        return makeError(ErrorCode::ParseFailure, "the answer is neither a STAC item nor a list of collections");
    }
    std::string root = baseOf(url);
    while (!root.empty() && root.back() == '/') {
        root.pop_back();
    }
    if (lower(root).ends_with("/collections")) {
        root.resize(root.size() - std::string("/collections").size());
    }
    OnlineProvider provider = makeProvider(url, hostOf(url) + " (STAC)");
    OnlineService service;
    service.id = "stac";
    service.title = provider.title;
    service.type = OnlineServiceType::Stac;
    service.endpoint = root + "/search";
    std::set<std::string> used;
    for (const Json& collection : json["collections"]) {
        if (!collection.is_object() || !collection.contains("id") || !collection["id"].is_string()) {
            continue;
        }
        std::set<std::string> assets;
        if (collection.contains("item_assets") && collection["item_assets"].is_object()) {
            for (const auto& [key, value] : collection["item_assets"].items()) {
                assets.insert(key);
            }
        }
        const std::string asset = assets.contains("visual") ? "visual"
                                  : assets.contains("red") && assets.contains("green") && assets.contains("blue")
                                      ? "red,green,blue"
                                      : std::string();
        if (asset.empty()) {
            continue;
        }
        const std::string id = collection["id"].get<std::string>();
        OnlineLayer layer = baseLayer(provider, service);
        layer.id = uniqueId(used, safeId(id));
        layer.title = collection.value("title", id);
        layer.kind = OnlineLayerKind::Imagery;
        layer.layerName = id;
        layer.asset = asset;
        if (collection.contains("license") && collection["license"].is_string()) {
            layer.licence = collection["license"].get<std::string>();
        }
        service.layers.push_back(std::move(layer));
    }
    if (service.layers.empty()) {
        return makeError(ErrorCode::NotFound,
                         "no STAC collection offers a visual asset or red, green and blue bands");
    }
    provider.services.push_back(std::move(service));
    return provider;
}

Result<OnlineProvider> discoverService(
    const std::string& url, const TextFetcher& fetch,
    const std::function<Result<OnlineProvider>(const std::string&)>& probeCog)
{
    const std::string folded = lower(url.substr(0, 8));
    if (!folded.starts_with("https://") && !folded.starts_with("http://") &&
        !folded.starts_with("file://")) {
        return makeError(ErrorCode::InvalidArgument, "a service address starts with https://",
                         katana::gis::redactUrl(url));
    }
    const auto guessed = guessServiceType(url);
    if (guessed == OnlineServiceType::Cog) {
        if (!probeCog) {
            return makeError(ErrorCode::Unsupported, "no way to read a COG header here");
        }
        return probeCog(url);
    }
    std::vector<OnlineServiceType> candidates;
    if (guessed) {
        candidates.push_back(*guessed);
    } else {
        candidates = {OnlineServiceType::Wms, OnlineServiceType::Wmts, OnlineServiceType::Wfs,
                      OnlineServiceType::ArcgisExport, OnlineServiceType::OgcApiFeatures};
    }
    std::string tried;
    std::optional<katana::core::Error> firstFailure;
    for (const OnlineServiceType type : candidates) {
        const std::string description = descriptionUrl(type, url);
        auto body = fetch(description);
        if (!body) {
            if (!firstFailure) {
                firstFailure = body.error();
            }
            tried += std::string(tried.empty() ? "" : ", ") + toString(type);
            continue;
        }
        Result<OnlineProvider> parsed = makeError(ErrorCode::Unsupported, "unreachable");
        switch (type) {
        case OnlineServiceType::Wms:
            parsed = parseWmsCapabilities(*body, url);
            break;
        case OnlineServiceType::Wmts:
            parsed = parseWmtsCapabilities(*body, url);
            break;
        case OnlineServiceType::Wfs:
            parsed = parseWfsCapabilities(*body, url);
            break;
        case OnlineServiceType::Wcs:
            parsed = parseWcsCapabilities(*body, url);
            break;
        case OnlineServiceType::ArcgisExport:
        case OnlineServiceType::ArcgisExportImage:
        case OnlineServiceType::ArcgisQuery:
            parsed = parseArcgisDescription(*body, url);
            break;
        case OnlineServiceType::OgcApiFeatures:
            parsed = parseOapifCollections(*body, url);
            break;
        case OnlineServiceType::Stac:
            parsed = parseStacDescription(*body, url);
            break;
        default:
            break;
        }
        if (parsed) {
            return parsed;
        }
        if (guessed) {
            return parsed.error();
        }
        tried += std::string(tried.empty() ? "" : ", ") + toString(type);
    }
    if (guessed && firstFailure) {
        return *firstFailure;
    }
    return makeError(ErrorCode::Unsupported,
                     "the address did not answer as any service Katana reads (tried " + tried + ")",
                     katana::gis::redactUrl(url));
}

} // namespace katana::interop
