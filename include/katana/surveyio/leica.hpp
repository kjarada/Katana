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
//     DistanceObservation, sharing one Pointing. The horizontal circle is
//     read as increasing clockwise: its direction is an instrument setting
//     (GSI ONLINE's SET/CONF 171) no word carries, and the import says so.
//     The FACE comes from the vertical reading: under half a turn is face
//     left, over it face right. A face-right zenith angle is stored in the
//     model's [0, pi] range, i.e. as a full turn less the reading (the
//     face-left equivalent); the horizontal direction is always the raw
//     circle reading. A block with a horizontal
//     distance (32) and no slope distance gives a horizontal distance; one
//     with a height difference (33) and neither a zenith angle nor a slope
//     distance gives a LevelDifferenceObservation from the station;
//   * every point the file names: with coordinates (81-83, or 84-86 when the
//     block measures nothing) as a SurveyPoint, without as an UnpositionedPoint;
//   * a point's code from word 71, else from its code block (41); remarks
//     72-79 and code information 42-49 in the point's metadata. Whether a
//     code block comes before or after its point is an INSTRUMENT SETTING
//     that GSI does not record: <Rec Free Code: Before Point / After Point>
//     (Leica TPS1200 Technical Reference Manual, version 5.0, 16.3 "Coding &
//     Linework Settings"). A file that begins with a point block and ends
//     with a code block has the shape only After Point gives, and its code
//     blocks code the point BEFORE them, with a warning; any other file's
//     code the point AFTER them. The import says which in project metadata
//     ("code blocks belong to") and in ReadResult::notCarried. A code block
//     with no point after it in a Before Point file goes to the point of the
//     block before it, with a warning;
//   * one SurveyFeature per run of consecutive points sharing a code.
//   * a setup's BACKSIGHT: GSI has no word for one. A setup is made on the
//     instrument as a station and then an orientation shot, recorded in that
//     order, so the first shot of a setup is its backsight when the file has
//     already given that point coordinates (a keyed-in control point, an
//     earlier station). A first shot to an unknown point names none.
// GSI states no coordinate system: the person supplies it, and the import says
// so (ReadResult::notCarried), as it says how many setups found a backsight.
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
