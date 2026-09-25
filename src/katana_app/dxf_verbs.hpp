#pragma once

// IMPORT and EXPORT of a .dxf, natively (katana_dxf, docs/dxf.md). Here and
// not in main.cpp so that the command line's one hook is a line, and
// here rather than behind the interoperability guard because reading a DXF
// needs no third-party library: a build without GDAL still has it.

#include <optional>
#include <string>
#include <string_view>

#include "katana/cad/document.hpp"
#include "katana/cad/import_placement.hpp"

namespace katana::app {

// `verb` is IMPORT or EXPORT, upper case; `path` the file, without
// surrounding quotes, and `placement` where an IMPORT puts what it reads
// (LOCAL, ALONGSIDE, OFFSET=dE,dN) - both as
// CommandInterpreter::importArgument reads the line. nullopt when the path is
// not a .dxf, so the caller carries on to its other importers; otherwise
// whether it worked, which has been reported on stdout or stderr.
[[nodiscard]] std::optional<bool> runDxfVerb(katana::cad::Document& document,
                                             std::string_view verb, const std::string& path,
                                             const katana::cad::ImportPlacement& placement);

} // namespace katana::app
