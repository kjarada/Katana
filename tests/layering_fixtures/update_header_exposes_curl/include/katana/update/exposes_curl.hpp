#pragma once

// A layering violation kept on purpose, as the fixture of the test
// layering_catches_an_update_header_exposing_curl (tests/CMakeLists.txt).
//
// A public header must not name libcurl (CLAUDE.md, section 11 says curl stays
// behind src/), and the update layer in particular never sees it: its network is
// injected (katana/update/feed.hpp). The check's pattern named neither this nor
// zlib nor OpenSSL until the update layer was added. The fixture is outside
// include/, where the real check never looks, and is never compiled.
#include <curl/curl.h>
