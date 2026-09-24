#pragma once

// RINEX observation files (versions 2.10/2.11, 3.0x and 4.0x): what an import
// of one produces. The reader itself registers from src/katana_surveyio/rinex.cpp
// (format.hpp gives the mechanism) and is called through readSurvey().
//
// A RINEX observation file is imported for what it says about an OCCUPATION,
// not for its raw measurements:
//   * one survey::GnssSession per occupation - normally one per file; a
//     stop-and-go file with "new site occupation" events has one per site -
//     with the receiver, the antenna and its height (vertical, to the antenna
//     reference point, as RINEX defines it), the first and last epoch, the
//     interval, the epoch count and the satellites seen per system;
//   * the marker as a survey::UnpositionedPoint named by MARKER NAME;
//   * the header's APPROX POSITION XYZ as a survey::GnssGlobalPositionObservation
//     of that marker, geocentric, with kRinexApproximatePositionSigma on each
//     axis and GnssSolution::Autonomous. It is an approximate position - most
//     often the receiver's own navigation solution - and is weighted so that
//     it can place the marker on a drawing but can never pull on a survey-grade
//     observation in an adjustment.
// The pseudorange, phase, Doppler and signal-strength values are checked and
// counted, never stored: processing raw GNSS is outside what Katana does.
//
// Hatanaka-compressed files (Compact RINEX 1.0 and 3.0, .YYd / .crx) are
// expanded as they are read and then read like any other; warnings name the
// compact file's own lines. gzip / Unix-compress / bzip2 / zip packed files
// are recognised and refused with the step that unpacks them.

#include <string_view>

namespace katana::surveyio {

// Stable: saved in survey jobs and source records.
inline constexpr std::string_view kRinexObservationFormatId = "rinex-observation";

// Metres, per ECEF axis, for a header's approximate position. A receiver's
// autonomous code solution is good to a few metres, and a header position may
// equally have been typed in roughly by whoever set the receiver up, so ten
// metres is the honest order of magnitude - and large enough that a
// millimetre-level observation always wins against it.
inline constexpr double kRinexApproximatePositionSigma = 10.0;

} // namespace katana::surveyio
