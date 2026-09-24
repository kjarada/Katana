#pragma once

// A SheetSet as versioned JSON: how a project stores its sheets, under the
// "sheets" key of its metadata, with no schema change (docs/plotting.md).
//
//   {"format": "katana-sheets", "version": 1,
//    "defaults": {...}, "numbering": "{n}", "revisions": [...],
//    "sheets": [{"id": "s1", "name": ..., "paper": "A3", "landscape": true,
//                "frame": "a3_landscape", "fields": {...},
//                "viewports": [{"id": "vp1", "kind": "plan",
//                               "rect": [x0, y0, x1, y1], "scale": 500, ...}]}]}
//
// Doubles are written in the shortest form that reads back to the same bits,
// so a save and a load give back exactly the set that was saved. A member
// left out reads as its default, so a later version can add members an older
// reader skips; a version newer than kSheetSetVersion is refused (Unsupported)
// rather than read wrongly and saved back without what it did not understand.

#include <string>
#include <string_view>

#include "katana/cad/plotting/sheet_set.hpp"
#include "katana/core/error.hpp"

namespace katana::cad::plotting {

// An unplaced viewport's empty rectangle is written as null. Fails
// (InvalidArgument) for text that is not valid UTF-8 and for a number that
// is not finite - neither has a JSON form that reads back.
[[nodiscard]] core::Result<std::string> sheetSetToJson(const SheetSet& set);

// ParseFailure for text that is not a sheet set; Unsupported for one written
// by a newer version.
[[nodiscard]] core::Result<SheetSet> sheetSetFromJson(std::string_view json);

} // namespace katana::cad::plotting
