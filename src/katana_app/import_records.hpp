#pragma once

// The records IMPORT and EXPORT reply with (docs/interop.md, "Replies"), in
// every build. The DXF verbs of a build without GDAL (dxf_verbs.cpp) and the
// geoprocessing executor's IMPORT and EXPORT (geo/import_verb.cpp,
// geo/export_verb.cpp) write the same words, so a script reads one reply
// whichever build ran it, and an agent reads the same fields from a DXF as
// from a GeoPackage.
//
//   imported file=site.dxf kind=dxf entities=9 bounds=0,0,30,20 ...
//   placed placement=local east=-180 north=0 text="LOCAL: moved as one piece by ..."
//   tally element=LINE read=3 imported=3
//   exported file=site.dxf kind=dxf driver=DXF entities=9 ...
//   warning text="..."

#include <filesystem>
#include <string>
#include <string_view>

#include "katana/cad/import_placement.hpp"
#include "katana/geometry/primitives2d.hpp"

namespace katana::app {

// A file's path as a record and a line give it: UTF-8, forward slashes. And
// back: a line's UTF-8 path as the path it names, whatever the code page.
[[nodiscard]] std::string pathText(const std::filesystem::path& path);
[[nodiscard]] std::filesystem::path pathFromText(std::string_view utf8);

// The rest of a line after its verb, blanks trimmed and one pair of
// surrounding quotes removed: EXPORT's and INFO's path, which works quoted
// or, holding blanks, not.
[[nodiscard]] std::string restOfLine(std::string_view line);

// A value as a record carries it: cad::recordValue - plain, or quoted when it
// holds a blank, a quote or an '=' - and nothing at all for nothing.
[[nodiscard]] std::string recordText(std::string_view value);

// A number in the shortest text that reads back exactly (core::formatExactReal).
[[nodiscard]] std::string recordNumber(double value);

// "x0,y0,x1,y1"; "" for an empty box, which has no corners to give.
[[nodiscard]] std::string boundsText(const katana::geometry::Box2& box);

// Where a placement put what was read, and the sentence that says so:
//   placed placement=alongside east=820 north=2000 text="ALONGSIDE: moved ..."
// east and north are what was ADDED to every coordinate, as OFFSET=dE,dN
// gives them, empty when nothing moved (ALONGSIDE into an empty drawing).
// "" for Keep, which moves nothing and says nothing.
[[nodiscard]] std::string placedRecord(const katana::cad::ImportPlacement& placement,
                                       const katana::cad::ImportShift& placed);

// tally element=<what> read=<n> imported=<n>: one kind of element a file
// held and how much of it came in.
[[nodiscard]] std::string tallyRecord(std::string_view element, std::size_t read,
                                      std::size_t imported);

// warning text="..."
[[nodiscard]] std::string warningText(std::string_view text);

} // namespace katana::app
