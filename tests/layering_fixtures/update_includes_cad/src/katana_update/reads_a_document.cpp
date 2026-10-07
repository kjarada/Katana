// A layering violation kept on purpose, as the fixture of the test
// layering_catches_the_update_layer_including_cad (tests/CMakeLists.txt).
//
// The update layer sees core and nothing else: the engine and the helper program
// that swaps the install's files must not drag the drawing model, GDAL or Qt into
// a program that has to stay small, static and independent of what it replaces.
// The fixture is outside src/, where the real check never looks, and is never
// compiled.
#include "katana/cad/document.hpp"
