#pragma once

// Leica field data: what an import of a Leica file produces. The readers
// register themselves from src/katana_surveyio/leica_gsi.cpp and leica_dbx.cpp
// (format.hpp gives the mechanism) and are called through readSurvey().
//
// LEICA GSI (GSI-8 and GSI-16, one format: a file may mix the two widths).
// The specification followed is Leica Geosystems, "GSI ONLINE for Leica TPS
// and DNA", November 2003: the data word on its pages 5-6, the word indices in
// the TPS1000/1100/2000/5000 PUT and GET tables on pages 35-37. One import
// produces:
//   * one survey::SurveyStation per setup: a block giving the station's
//     coordinates (words 84-86) and/or the instrument height (88) and no
//     measurement, FOLLOWED by measurement blocks. Its InstrumentSettings carry
//     the ppm (51/59) and prism constant (51/58) the instrument recorded -
//     Applied, because a Leica instrument records the settings it applied to
//     the distances - and the serial number (12), the instrument type (13) and
//     the time (18 + 19) where the file records them;
//   * per measurement block, one pointing: the horizontal circle reading (21)
//     as a HorizontalDirectionObservation, the vertical circle reading (22) as a
//     ZenithAngleObservation and the slope distance (31) as a
//     DistanceObservation, sharing one Pointing. The FACE comes from the
//     vertical reading: under half a turn is face left, over it face right. A
//     face-right zenith angle is stored in the model's [0, pi] range, i.e. as
//     a full turn less the reading (the face-left equivalent); the horizontal
//     direction is always the raw circle reading. A block with a horizontal
//     distance (32) and no slope distance gives a horizontal distance; one
//     with a height difference (33) and neither a zenith angle nor a slope
//     distance gives a LevelDifferenceObservation from the station;
//   * every point the file names: with coordinates (81-83, or 84-86 when the
//     block measures nothing) as a SurveyPoint, without as an UnpositionedPoint;
//   * a point's code from word 71, else from the code block (41) BEFORE it;
//     remarks 72-79 and code information 42-49 in the point's metadata;
//   * one SurveyFeature per run of consecutive points sharing a code.
// GSI states no coordinate system and marks no backsight: both are left for the
// person to supply, and the import says so (ReadResult::notCarried).
//
// LEICA DBX is Leica's internal job database (an index .xcf and data files
// .x01, .x02 ... in one folder). No specification of it is published, so it is
// RECOGNISED and REFUSED with the export to make instead; it is never guessed at.

#include <string_view>

namespace katana::surveyio {

// Stable: saved in survey jobs and source records.
inline constexpr std::string_view kLeicaGsiFormatId = "leica-gsi";
inline constexpr std::string_view kLeicaDbxFormatId = "leica-dbx";

} // namespace katana::surveyio
