#pragma once

// A SheetSet as versioned JSON: how a project stores its sheets, under the
// "sheets" key of its metadata, with no schema change (docs/plotting.md).
//
//   {"format": "katana-sheets", "version": 1,
//    "defaults": {...}, "numbering": "{set}-{n:02}", "revisions": [...],
//    "sheets": [{"id": "s1", "name": "PLAN", "paper": "A1",
//                "viewports": [{"id": "vp1", "kind": "plan",
//                               "rect": [x0, y0, x1, y1], "scale": 1000, ...}]}]}
//
// Only the format, the version and every sheet's and viewport's id (and a
// viewport's kind) are always written; any other member is written only when
// it differs from the default of its type (a default-constructed Sheet,
// Viewport, SheetSet), and a member left out reads as that default. That
// keeps a generated set - hundreds of viewports differing in a few members -
// small, and small is fast to store and read. So a default never changes
// without a version increase.
//
// Doubles are written in the shortest form that reads back to the same bits,
// so a save and a load give back exactly the set that was saved. A later
// version can add members an older reader skips; a version newer than
// kSheetSetVersion is refused (Unsupported) rather than read wrongly and
// saved back without what it did not understand.

#include <string>
#include <string_view>

#include "katana/cad/plotting/sheet_set.hpp"
#include "katana/core/error.hpp"

namespace katana::cad::plotting {

// Fails (InvalidArgument) for text that is not valid UTF-8 and for a number
// that is not finite - neither has a JSON form that reads back. (An unplaced
// viewport's empty rectangle is the default, and is simply left out.)
[[nodiscard]] core::Result<std::string> sheetSetToJson(const SheetSet& set);

// ParseFailure for text that is not a sheet set; Unsupported for one written
// by a newer version.
[[nodiscard]] core::Result<SheetSet> sheetSetFromJson(std::string_view json);

} // namespace katana::cad::plotting
