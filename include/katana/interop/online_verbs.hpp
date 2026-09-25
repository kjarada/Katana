#pragma once

// The ONLINE verbs (docs/gis_online.md, "Verbs"): what an agent, a script or a
// person types to do everything GIS > Online Data does.
//
//   ONLINE PROVIDERS [filter]
//   ONLINE LAYERS <provider> [search words, for a catalogue provider]
//   ONLINE INFO <provider> <layer>
//   ONLINE IMPORT <provider> <layer> area=view|drawing|selection|x0,y0,x1,y1|lonlat:w,s,e,n
//                 [res=<units per pixel>] [layer=<target layer>] [from=YYYY-MM-DD]
//                 [to=YYYY-MM-DD] [cloud=<percent>] [tag=<key[=value]>] [time=<date>]
//                 [crs=EPSG:<code>] [timeout=<seconds>]
//   ONLINE CUSTOM <url>
//   ONLINE KEY <name> <value>      (ONLINE KEY <name> alone removes it)
//
// The grammar and the replies live here, beside the fetcher and away from Qt,
// so that they are tested as text and so that every front end speaks them the
// same way; the window executes them (src/katana_qt/gis_online.cpp).
//
// Replies are for machines first: one record per line, a word saying what the
// record is, then key=value fields in a fixed order, a value quoted with ""
// when it holds a space, a quote or an equals sign (the Logger's rule,
// docs/architecture.md "Logging"), so `split on spaces outside quotes` reads
// every line.

#include <optional>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/gis/reproject.hpp"
#include "katana/interop/online_catalogue.hpp"
#include "katana/interop/online_fetch.hpp"

namespace katana::interop {

enum class OnlineVerb { Providers, Layers, Info, Import, Custom, Key };

enum class OnlineAreaKind { View, Drawing, Selection, Box };

struct OnlineCommand {
    OnlineVerb verb = OnlineVerb::Providers;
    std::string filter;   // PROVIDERS; LAYERS' search words
    std::string provider; // LAYERS, INFO, IMPORT
    std::string layer;    // INFO, IMPORT
    std::string url;      // CUSTOM
    std::string keyName;  // KEY
    std::string keyValue; // KEY; empty removes
    // IMPORT
    OnlineAreaKind area = OnlineAreaKind::View;
    katana::gis::CrsBox box;     // Box: the typed corners
    bool boxIsLonLat = false;    // lonlat: prefix - WGS 84 degrees, not the project's CRS
    std::optional<double> resolution;
    std::string targetLayer;
    std::string fromDate;
    std::string toDate;
    std::optional<double> maxCloud;
    std::string tag;
    std::string time;
    std::string crs;
    std::optional<int> timeoutSeconds;
};

// Parses a line that starts with ONLINE (case-insensitive). InvalidArgument
// with the usage of the verb for anything malformed: an unknown option, a
// box that is not four numbers with min below max, a date that is not
// YYYY-MM-DD, a cloud ceiling outside 0 to 100, a non-positive resolution.
// Values may be quoted ("layer=\"my layer\"").
[[nodiscard]] katana::core::Result<OnlineCommand> parseOnlineCommand(std::string_view line);

// The usage text, every verb.
[[nodiscard]] std::string onlineUsage();

// A value as a reply field: quoted when it holds whitespace, '"' or '=',
// with '"' and '\' escaped.
[[nodiscard]] std::string replyValue(std::string_view value);

// ---- replies -------------------------------------------------------------------------

// "provider id=... group=... layers=N title=..." per provider, after a
// "providers count=N" line.
[[nodiscard]] std::string formatProviders(const std::vector<const OnlineProvider*>& providers);
// "layer provider=... id=... kind=... service=... key=yes|no verified=DATE|no
// title=..." per layer, after "layers provider=... count=N".
[[nodiscard]] std::string formatLayers(const OnlineProvider& provider);
// "info provider=... layer=..." then one "field name=value" line per fact,
// in a fixed order: title, kind, service, endpoint (key redacted), layer,
// crs, coverage, resolution, limits, licence, attribution, key, verified,
// evidence.
[[nodiscard]] std::string formatInfo(const OnlineLayer& layer);
// The outcome of an import: "imported provider=... layer=... kind=..." with
// what landed (entities and target layer, or raster size and resolution),
// the requests, cache hits, pages and repeats dropped, and the licence,
// attribution and source; each warning on its own "warning text=..." line.
[[nodiscard]] std::string formatImport(const OnlineLayer& layer, const OnlineImport& result,
                                       std::size_t entitiesAdded, const std::string& targetLayer);
// "error verb=... code=... message=... context=...", the one shape every
// refusal takes.
[[nodiscard]] std::string formatError(OnlineVerb verb, const katana::core::Error& error);
// Catalogue search results (ONLINE LAYERS data-gov-au <words>).
[[nodiscard]] std::string formatCatalogueSearch(const std::vector<CkanResource>& resources);

[[nodiscard]] const char* toString(OnlineVerb verb);

} // namespace katana::interop
