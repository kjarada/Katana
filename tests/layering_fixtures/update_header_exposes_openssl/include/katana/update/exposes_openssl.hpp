#pragma once

// A layering violation kept on purpose, as the fixture of the test
// layering_catches_an_update_header_exposing_openssl (tests/CMakeLists.txt).
//
// A public header of the update layer must not name OpenSSL: it stays behind src/,
// so that what the engine verifies signatures with can change without touching
// anything that includes the engine. The fixture is outside include/, where the
// real check never looks, and is never compiled.
#include <openssl/evp.h>
