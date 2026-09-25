#pragma once

// What an alignment reports: its setting-out table and its design profile's
// elements (docs/cad.md, "The Alignment Manager").
//
// ONE function per report, read by both front ends: ALIGN STATIONS and ALIGN
// PROFILE print these, and Terrain > Alignment Manager shows them in its
// tables. A number that appeared only in the dialog would be a second answer
// nobody tests; a station the dialog listed and the verb did not would be
// worse.
//
// Nothing is stored. Like a parcel's report, the table is worked out from the
// solved alignment when asked for, so it cannot disagree with the PIs it came
// from.

#include <optional>
#include <string>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/geometry/alignment.hpp"
#include "katana/geometry/primitives2d.hpp"
#include "katana/geometry/profile.hpp"

namespace katana::cad {

// One row of a setting-out table.
struct SettingOutStation {
    double station = 0.0;
    geometry::Point2 point;
    // Radians counter-clockwise from +x, as SolvedAlignment gives it.
    double direction = 0.0;
    // The same direction as a surveyor reads it: radians clockwise from grid
    // north, in [0, 2 pi).
    double azimuth = 0.0;
    // Signed, positive turning left; 0 on a tangent. On a spiral it is the
    // curvature at this station, which is what the peg there is set out on.
    double curvature = 0.0;
    // What the station is, when it is where the geometry changes character:
    // "start" and "end"; "TS", "SC", "CS", "ST" through a spiralled curve;
    // "TC" and "CT" either side of a simple curve; "PI" at a corner with no
    // curve; the two letters of the elements met ("SS", "CC") where two
    // curves run back to back. Empty for a station that is on the interval
    // only.
    std::string key{};
};

// Every station `interval` apart from the alignment's start, and every key
// station (geometry::SolvedAlignment::keyStations), ascending, each once: a
// key station the interval lands on is one row, carrying its key. A
// setting-out table without the TS, SC, CS and ST is not one - those are the
// points that get pegged - so the key stations are always there.
//
// Fails with InvalidArgument for an interval that is not positive and finite,
// and for one that would give more than 100 000 stations (a mistyped 0.001
// on a 20 km road), naming the count.
[[nodiscard]] core::Result<std::vector<SettingOutStation>>
settingOutStations(const geometry::SolvedAlignment& alignment, double interval);

// An azimuth (radians clockwise from north) in degrees, minutes and whole
// seconds, the precision a setting-out sheet quotes: 61°21'08". What the
// reply, the CSV and the Alignment Manager's table all print.
[[nodiscard]] std::string formatAzimuth(double azimuth);

// The reply of ALIGN STATIONS: a key=value record per station,
//   station=128.540 x=100.000 y=50.000 direction=90.000 azimuth=0°00'00" radius=straight key=ST
// to three decimals; `direction` in degrees counter-clockwise from +x, the
// azimuth in whole seconds; `radius=50.000 turn=left` on a curve. The key is
// left out on a station that has none, as absent is not blank.
[[nodiscard]] std::string formatSettingOut(const std::vector<SettingOutStation>& stations);

// The table as CSV (RFC 4180: CRLF, quoted where a field needs it) with a
// header row - chainage, easting, northing, the azimuth in decimal degrees
// and in degrees, minutes and seconds, the radius (blank on a tangent,
// negative turning right) and the key - for a field book or a spreadsheet.
[[nodiscard]] std::string settingOutCsv(const std::vector<SettingOutStation>& stations);

// The K value of a vertical curve: its length over the algebraic difference
// of its grades in percent, the metres of curve per 1% change of grade that
// road standards set minimums on. nullopt for a tangent, or a curve whose
// grades do not differ.
[[nodiscard]] std::optional<double> curveK(const geometry::ProfileElement& element);

// The reply of ALIGN PROFILE: the PVIs, then each element with its grades
// (and K on a curve), then the high and low points.
[[nodiscard]] std::string formatProfileReport(const geometry::VerticalAlignment& definition,
                                              const geometry::SolvedProfile& profile);

} // namespace katana::cad
