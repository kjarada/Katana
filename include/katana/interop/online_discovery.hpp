#pragma once

// What a web service offers, found from its own description: GIS > Online
// Data > Add Custom Service... and the ONLINE CUSTOM verb (docs/gis_online.md,
// "Custom services").
//
// A person pastes a URL; Katana asks the service to describe itself - a WMS,
// WMTS or WFS GetCapabilities document, an ArcGIS REST service's JSON, an OGC
// API's collections, a STAC API's collections or a single STAC item, a COG's
// header - and turns the answer into an OnlineProvider with one layer per
// thing it can import, in the same form the built-in catalogue has. From
// there a custom layer is imported exactly as a built-in one is.
//
// The parsers are pure functions of the document, tested on committed
// capabilities fixtures (tests/interop/data/online/); `discoverService` adds
// only the fetching, through a function the caller supplies, so the tests
// run it over file:// URLs.

#include <functional>
#include <stop_token>
#include <string>
#include <string_view>

#include "katana/core/error.hpp"
#include "katana/interop/online_catalogue.hpp"

namespace katana::interop {

// The kind of service a URL most probably is, from its shape alone: an
// ArcGIS REST path (/MapServer, /ImageServer, /FeatureServer), a service=
// parameter or a capabilities file name, a .tif, a STAC or OGC API path.
// Nullopt when the URL does not say; discovery then tries the candidates in
// turn.
[[nodiscard]] std::optional<OnlineServiceType> guessServiceType(std::string_view url);

// The address of the description a service of `type` publishes at `url`:
// GetCapabilities for the OGC services, ?f=json for ArcGIS REST, /collections
// for OGC API and STAC, the URL itself for a STAC item or COG.
[[nodiscard]] std::string descriptionUrl(OnlineServiceType type, const std::string& url);

// ---- parsers: a description document -> a provider --------------------------

// The id a discovered provider gets: "custom-" and the host, made safe for a
// verb and a file name, e.g. custom-opendata-maps-vic-gov-au.
[[nodiscard]] std::string customProviderId(std::string_view url);

[[nodiscard]] katana::core::Result<OnlineProvider>
parseWmsCapabilities(std::string_view xml, const std::string& url);
[[nodiscard]] katana::core::Result<OnlineProvider>
parseWmtsCapabilities(std::string_view xml, const std::string& url);
[[nodiscard]] katana::core::Result<OnlineProvider>
parseWfsCapabilities(std::string_view xml, const std::string& url);
[[nodiscard]] katana::core::Result<OnlineProvider>
parseWcsCapabilities(std::string_view xml, const std::string& url);
// A MapServer, ImageServer or FeatureServer description, or one layer of a
// MapServer or FeatureServer (…/MapServer/9?f=json).
[[nodiscard]] katana::core::Result<OnlineProvider>
parseArcgisDescription(std::string_view json, const std::string& url);
[[nodiscard]] katana::core::Result<OnlineProvider>
parseOapifCollections(std::string_view json, const std::string& url);
// A STAC API's /collections, or a single STAC item (which becomes a layer
// of its own: the item's visual asset, or red/green/blue composed).
[[nodiscard]] katana::core::Result<OnlineProvider>
parseStacDescription(std::string_view json, const std::string& url);

// ---- discovery -------------------------------------------------------------------

// Fetches text, for discovery: a URL in, the body out. online_fetch.hpp's
// fetchText is the one used in the program, with its cache and User-Agent.
using TextFetcher = std::function<katana::core::Result<std::string>(const std::string& url)>;

// Asks the service at `url` to describe itself and returns what it offers,
// every layer marked userDefined. The type is guessed from the URL, and when
// the URL does not say, WMS, then WMTS, WFS and ArcGIS REST are tried in turn.
// A COG (.tif) is probed through `probeCog` (gis::probeRaster over /vsicurl/),
// since its header is its description. Unsupported, naming what was tried,
// when nothing answers as a service Katana reads; the service's own
// failure otherwise.
[[nodiscard]] katana::core::Result<OnlineProvider>
discoverService(const std::string& url, const TextFetcher& fetch,
                const std::function<katana::core::Result<OnlineProvider>(const std::string&)>& probeCog = {});

} // namespace katana::interop
