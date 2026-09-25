#pragma once

// A delimited schedule of located utility vertices, and a design centre line,
// read from text.
//
// One row per located vertex; rows with the same `line` form one service, in
// the order they appear. Columns are found BY NAME from a header row, in any
// order and any letter case, so that a schedule exported from a locating
// contractor's spreadsheet reads without being rearranged first:
//
//   required  line, point, easting, northing, method
//   optional  level, level_ref, surface, h_unc, v_unc, ql, path, verifies,
//             type, owner, material, diameter_mm, status, config, description
//
// The TfNSW Utility Schema's attribute names are accepted too - AssetIdentifier
// for line, AssetTypeCode for type, LocateMethod, DepthLocation, Depth, Size
// (millimetres, inside, "W x H") - and the rest of its attributes are kept
// by name, uninterpreted (docs/subsurface_utilities.md has the table).
//
// Each name has aliases (utilityCsvColumns() lists them), and a column whose
// header matches none of them is an error rather than something skipped: a
// misspelt "survace" column read past would leave every cover uncomputed with
// nothing to say why.
//
// Lengths and levels are metres, except diameter_mm, whose unit is in its name
// because a diameter in a utility schedule is millimetres by habit and metres
// everywhere else in this program - the one column where guessing would be
// wrong by a thousand.
//
// The attributes of a service (type, owner, material ...) may be repeated on
// every row or given once; two rows of one line that give DIFFERENT values
// are an error naming both rows. An empty cell is "not recorded", never zero.
//
// The text is UTF-8 (an optional BOM is skipped). Fields are separated by
// commas; a field may be double-quoted, with "" for a quote inside it. Blank
// lines and lines starting with '#' are skipped.

#include <string>
#include <string_view>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/survey/subsurface/clearance.hpp"
#include "katana/survey/subsurface/utility_network.hpp"

namespace katana::survey::subsurface {

// Where a column that is kept but not interpreted is kept.
enum class CarriedOn {
    Interpreted, // read into the model's own fields
    Line,        // UtilityAttributes::fields, one value per service
    Vertex,      // UtilityVertex::fields
};

struct UtilityCsvColumn {
    std::string_view name;                 // the canonical name, as listed above
    std::vector<std::string_view> aliases; // also accepted, compared without case, blanks, '-', '_'
    bool required = false;
    CarriedOn carried = CarriedOn::Interpreted;
};

// Every column the reader knows, in the order above.
[[nodiscard]] const std::vector<UtilityCsvColumn>& utilityCsvColumns();

// Lines in order of first appearance. ParseFailure, naming the line of text,
// for: no header, an unknown or repeated column, a missing required column,
// a row with the wrong number of fields, an unparseable number / method /
// quality level / type / status / level reference / path, a non-positive
// diameter, a line id repeated for the same point id, or conflicting line
// attributes.
[[nodiscard]] core::Result<std::vector<UtilityLine>> parseUtilityCsv(std::string_view text);

// A design centre line: header row with easting, northing and optionally
// level (aliases as above), one row per vertex in order. `id` names the
// alignment in results and errors.
[[nodiscard]] core::Result<DesignAlignment> parseDesignCsv(std::string_view text, std::string id);

} // namespace katana::survey::subsurface
