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

#include <functional>
#include <map>
#include <string>
#include <string_view>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/survey/subsurface/clearance.hpp"
#include "katana/survey/subsurface/delivery_schema.hpp"
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

// The column `header` names, by its name or an alias, compared as the reader
// compares a header row's; null for a name it does not know. So a column
// named anywhere else - UTILITY DRAW's FIELDS, which reads an import's own
// attributes as columns - is the one the reader would take.
[[nodiscard]] const UtilityCsvColumn* utilityCsvColumnNamed(std::string_view header);

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

// ---- writing a schedule --------------------------------------------------------------------
//
// The other direction, for a schedule made from the drawing (UTILITY SCHEDULE,
// docs/subsurface_utilities.md): a drawing edited in CAD becomes a
// deliverable again, in the format the reader above takes.

// The words a schedule is written in. Empty, the reader's own: each column
// under its canonical name, each value as toString spells it. A client's
// delivery schema names some columns and spells some values its own way
// (utilityCsvDialect); those are what the maps hold.
struct UtilityCsvDialect {
    // Canonical column name -> the header written for it. Each header must be
    // one the reader takes for that column: the name or one of its aliases.
    std::map<std::string, std::string, std::less<>> headers;
    // Canonical column name -> (the value as the reader's own spelling writes
    // it -> the value written). Only for the columns whose values are words:
    // type, status, method, level_ref, ql and path. Each spelling must read
    // back as the same value.
    std::map<std::string, std::map<std::string, std::string, std::less<>>, std::less<>> spellings;
};

// `lines` as a schedule that parseUtilityCsv reads back as `lines`, field for
// field and doubles to the bit (UtilityLine's ==): one row per vertex in
// order, the service's attributes on every row of it, numbers in the shortest
// text that reads back exactly (core::formatExactReal), a diameter in the
// millimetres its column is in. A column is written only when some row has a
// value for it, except the five required ones. A quality level claimed is
// written as claimed; a vertex claiming none leaves the cell empty, which the
// reader reads as none.
//
// A diameter in metres that no millimetre figure divides back to exactly - one
// typed in metres rather than read from a schedule - is written to the nearest
// millimetre text there is, and reads back a unit in the last place away.
//
// InvalidArgument, naming the line and vertex, for what a schedule cannot
// hold: a value with a line break in it; a kept attribute whose name is no
// column the reader knows, or is carried by the other of a service and a
// vertex (utilityCsvColumns); a pathEvidence list of the wrong size.
[[nodiscard]] core::Result<std::string> writeUtilityCsv(const std::vector<UtilityLine>& lines,
                                                        const UtilityCsvDialect& dialect = {});

// The dialect of a client's delivery schema: each column the reader knows
// under the schema attribute that is one of its names (an attribute or label
// equal to the column's name or an alias, ignoring case, blanks, '-' and
// '_'), and each word value in the FIRST spelling the schema's domain lists
// that reads as it. So a schedule written from the drawing is written as the
// schema asks, and a value the schema has no spelling for is written in the
// reader's own - for a check to find.
[[nodiscard]] UtilityCsvDialect utilityCsvDialect(const DeliverySchema& schema);

} // namespace katana::survey::subsurface
