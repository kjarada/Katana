#pragma once

// The update engine's network, from the project's one HTTP stack.
//
// The engine (katana_update) links no HTTP library: it takes a `Fetch` (update/feed.hpp) as a
// parameter. This is the Fetch the programs pass: update::makeFetch - the engine's own rules for
// redirects, the access token, portals and ranges, which its tests hold to the letter - over
// gis::httpExchange, one request on a libcurl handle that does not follow a redirect (GDAL's
// CPLHTTPFetch cannot be told not to). The proxy, the certificate bundle and the timeouts are
// GDAL's, as for every other web request Katana makes (docs/gis_online.md).
//
// Without KATANA_WITH_INTEROP (-DKATANA_BUILD_IO=OFF) there is no network layer: the Fetch it
// returns answers every request Unsupported, so check() reports "unreachable" and nothing else
// in the build changes (session.cpp does the same for the network verbs).
//
// The token is whatever the request carries in FetchRequest::bearerToken; this file never
// reads, stores or logs one.

#include "katana/update/feed.hpp"

namespace katana::app {

[[nodiscard]] update::Fetch makeUpdateFetch();

} // namespace katana::app
