#pragma once

// Where imported data lands (docs/interop.md, "Placing an import"): at its own
// coordinates, moved so its lower-left corner sits at 0,0, moved onto the
// drawing, or moved by a given offset. One definition for every importer and
// every front end - the window's IMPORT line and its import dialogs,
// katana_cli's and katana_mcp's IMPORT - so a placement cannot mean one thing
// typed and another chosen in a dialog.
//
// Here, in the CAD core, because both front ends read the word off an IMPORT
// line (CommandInterpreter::importArgument) and the native DXF import needs it
// in a build without GDAL, where interop's advisePlacement does not exist.
//
// The readers take the move as an origin shift they SUBTRACT from every
// coordinate (VectorImportOptions::originShift and its kin), so the file is
// read again with it rather than moved afterwards: one reader applies one
// shift to every kind of geometry it holds.

#include <optional>
#include <string>
#include <string_view>

#include "katana/core/error.hpp"
#include "katana/geometry/primitives2d.hpp"

namespace katana::cad {

enum class ImportPlacementMode {
    Keep,      // its own coordinates (the window still asks when it lands far away)
    Local,     // lower-left corner to 0,0: LOCAL
    Alongside, // lower-left corner onto the drawing's: ALONGSIDE
    Offset,    // moved by a given east and north: OFFSET=dE,dN
};

struct ImportPlacement {
    ImportPlacementMode mode = ImportPlacementMode::Keep;
    // For Offset: what is ADDED to every coordinate, east then north.
    katana::geometry::Vec2 offset{};

    friend bool operator==(const ImportPlacement&, const ImportPlacement&) = default;
};

// The word an IMPORT line ends in for `placement`: "LOCAL", "ALONGSIDE",
// "OFFSET=dE,dN" (each number the shortest text that reads back exactly), or
// "" for Keep, which needs none.
[[nodiscard]] std::string placementWord(const ImportPlacement& placement);

// The placement `word` names, in any case: LOCAL, ALONGSIDE or OFFSET=dE,dN.
// nullopt when it names none, so the word is part of a file name;
// InvalidArgument for an OFFSET= whose value is not two finite numbers, which
// is a mistyped placement and not a file.
[[nodiscard]] katana::core::Result<std::optional<ImportPlacement>>
parsePlacementWord(std::string_view word);

// What a placement does to one import, worked out once the data has been
// read at its own coordinates.
struct ImportShift {
    // The origin shift to read the data again with - subtracted from every
    // coordinate - or nullopt when it stays where it is: Keep; Alongside into
    // an empty drawing, which has nothing to sit beside; and data with no
    // extent, which has nothing to move.
    std::optional<katana::geometry::Vec2> shift;
    // What the log says it did, or why it did nothing; "" for Keep.
    std::string said;
};

// `drawing` is the drawing's extent before the import, `incoming` the data's
// at its own coordinates. Alongside is the move the far-apart question's
// Shift Alongside makes (interop::advisePlacement's suggestedShift): the
// data's lower-left corner onto the drawing's.
[[nodiscard]] ImportShift resolveImportShift(const ImportPlacement& placement,
                                             const katana::geometry::Box2& drawing,
                                             const katana::geometry::Box2& incoming);

} // namespace katana::cad
