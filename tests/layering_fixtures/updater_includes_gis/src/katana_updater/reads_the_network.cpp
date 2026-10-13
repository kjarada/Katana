// A layering violation kept on purpose, as the fixture of the test
// layering_catches_the_updater_program_including_gis (tests/CMakeLists.txt).
//
// The helper program in src/katana_updater is the update layer's, which sees core
// and nothing else: its network is injected (katana/update/feed.hpp), so it can
// never need gis::httpFetch - which lives in the library that links GDAL. The
// fixture is outside src/, where the real check never looks, and is never
// compiled.
#include "katana/gis/web_access.hpp"
