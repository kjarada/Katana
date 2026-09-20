#pragma once

// Formatting a measured distance for a dimension label (PLAN.MD Phase 09).
//
// This is a number that appears on a drawing somebody signs, so the rules are
// pinned rather than left to whatever the platform's printf does.
//
// ORDER OF OPERATIONS is scale, then round, then fix the decimals - DIMLFAC,
// DIMRND, DIMDEC in AutoCAD's names. Rounding before scaling rounds in the
// wrong units and gives a different number: 1.24 m rounded to 0.05 is 1.25,
// but scaled to millimetres first it is 1240 and rounding THAT to 0.05 does
// nothing at all.
//
// TIES GO AWAY FROM ZERO, which is what a surveyor expects and what AutoCAD
// does: 2.5 at zero decimals is 3, not 2. That is why the rounding goes through
// std::llround, which C17 7.12.9.6-7 specifies to round halfway cases away from
// zero REGARDLESS of the current rounding mode. printf, std::to_chars with a
// fixed precision, and QString::number all round ties to even instead, so none
// of them may be used here.
//
// THE OUTPUT IS LOCALE-INDEPENDENT. std::to_chars on the integer parts and
// hand-emitted digits for the fraction; never printf or iostreams, either of
// which would put a comma in "1,5" on a European machine and produce a drawing
// that reads as a different number. The parsing side already states this rule
// (command_interpreter.cpp).
//
// UNITS ARE A SCALE FACTOR AND A SUFFIX, never a unit enum. The entity layer may
// only see core, math and geometry (tools/check_layering.cmake), so
// katana/geodesy/units.hpp is unreachable from here - and duplicating it would
// be a second definition of a foot.

#include <string>

#include "katana/core/error.hpp"

namespace katana::entity {

struct DimensionStyle;

// Formats `measurement` (model units, never negative) under `style`.
//
// Never fails: a style that reached the model has been validated, and a
// dimension label that could not be produced would leave a blank on a drawing
// with nothing to say why. Degenerate inputs are handled explicitly instead -
// see the tests for what each produces.
[[nodiscard]] std::string formatMeasurement(double measurement, const DimensionStyle& style);

// The same, for a dimension that carries its own text. An override wins
// VERBATIM: no prefix, no suffix, no rounding and no unit scale, because DXF
// does not apply DIMPOST to overridden text and a surveyor who typed a value
// means that value.
[[nodiscard]] std::string dimensionLabel(double measurement, const std::string& textOverride,
                                         const DimensionStyle& style);

} // namespace katana::entity
