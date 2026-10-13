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
// returns answers every request Unsupported with the token `no_network`, so check() reports
// "unreachable reason=no_network" and nothing else in the build changes (session.cpp does the
// same for the network verbs).
//
// The token is whatever the request carries in FetchRequest::bearerToken; this file never
// reads, stores or logs one. What gis::httpExchange says about a failure is not passed on as it
// is: its contexts show an address with its query, and the address a release redirects to is a
// signed one. Every Error leaves with the address through update::displayUrl.
//
// This file is the one part of the update system that needs a socket, so no unit test is of it.
// The rules it must keep - where the token goes, which redirects are followed, what an answer
// must be - are update::makeFetch's and are tested there over a recorded wire
// (tests/update/test_feed.cpp); the end-to-end tests are what drive this file, against a feed
// on the loopback address (tests/update/CMakeLists.txt, "the end-to-end tests").

#include "katana/update/feed.hpp"

namespace katana::app {

[[nodiscard]] update::Fetch makeUpdateFetch();

} // namespace katana::app
