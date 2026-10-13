#pragma once

// The update engine's network, from the project's one HTTP stack.
//
// The engine (katana_update) links no HTTP library: it takes a `Fetch` (update/feed.hpp) as a
// parameter. This is the Fetch the programs pass: update::makeFetch - the engine's own rules for
// redirects, the access token, portals and ranges, which its tests hold to the letter - over
// gis::httpExchange, one request on a libcurl handle that does not follow a redirect (GDAL's
// CPLHTTPFetch cannot be told not to). The proxy, the certificate bundle and the timeouts are
// GDAL's, as for every other web request Katana makes (docs/gis_online.md); the credentials a
// person configured for GDAL's requests (a .netrc entry, GDAL_HTTP_USERPWD, GDAL_HTTP_HEADERS,
// cookies) are NOT: an exchange sends the headers it is given and the User-Agent, no more.
//
// Without KATANA_WITH_INTEROP (-DKATANA_BUILD_IO=OFF) there is no network layer: the Fetch it
// returns answers every request Unsupported with the token `no_network`, so check() reports
// "unreachable reason=no_network" and nothing else in the build changes (session.cpp does the
// same for the network verbs).
//
// The token is whatever the request carries in FetchRequest::bearerToken; this file never
// reads, stores or logs one. What gis::httpExchange says about a failure is not passed on as it
// is: its contexts show an address with its query, and the address a release redirects to is a
// signed one. Of them this file passes on libcurl's own words alone; update::makeFetch writes
// the address of every Error itself, through update::displayUrl, and cuts the query from any
// address a hop quoted all the same.
//
// This file is the one part of the update system that needs a socket, and NO TEST DRIVES IT.
// The rules it must keep - where the token goes, which redirects are followed, what an answer
// must be - are update::makeFetch's and are tested there over a recorded wire
// (tests/update/test_feed.cpp); the one request it makes is gis::httpExchange's, which has
// tests of its own (tests/interop/test_online_fetch.cpp, and test_online_requests.cpp beside
// it). The end-to-end tests do NOT come through here: their program links the engine and no
// libcurl, and carries update::makeFetch over a hop of its own on plain sockets
// (tests/update/e2e_main.cpp), because a program that ships cannot be given the test key or
// a feed on the loopback address. What is left to this file - handing a request to the
// exchange and its answer back - first meets a server in a check against a real release
// (docs/update.md, "To verify on first release").

#include "katana/update/feed.hpp"

namespace katana::app {

[[nodiscard]] update::Fetch makeUpdateFetch();

} // namespace katana::app
