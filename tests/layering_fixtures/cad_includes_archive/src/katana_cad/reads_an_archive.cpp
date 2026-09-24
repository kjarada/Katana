// A layering violation kept on purpose, as the fixture of the test
// layering_catches_a_cad_file_including_archive12d (tests/CMakeLists.txt).
//
// cad may not see archive12d. The check's include pattern once matched module
// names of letters alone, so this line - the include of a module whose name
// has digits - passed unseen. The fixture is outside src/, where the real
// check never looks, and is never compiled.
#include "katana/archive12d/archive.hpp"
