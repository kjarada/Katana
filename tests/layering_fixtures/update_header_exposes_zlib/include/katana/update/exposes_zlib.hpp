#pragma once

// A layering violation kept on purpose, as the fixture of the test
// layering_catches_an_update_header_exposing_zlib (tests/CMakeLists.txt).
//
// A public header of the update layer must not name zlib: it stays behind src/.
// The fixture is outside include/, where the real check never looks, and is never
// compiled.
#include <zlib.h>
