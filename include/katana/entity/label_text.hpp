#pragma once

// The label template language (docs/annotation.md, "Templates").
//
// A label style's text is literal text with fields in braces:
//
//     {bearing:dms} {distance:.3f}
//     {area:m2:.1f} m2\n{area:ha:.4f} ha
//     RL {z:.3f}
//
// A field is a VALUE name and then any number of ':'-separated steps applied
// left to right: unit conversions (m, mm, km, ft; m2, ha, km2, ac; deg, rad,
// gon), a number format (.Nf, N decimals), an angle format (dms, dms.N for N
// decimals of a second, dm, qb for a quadrant bearing), the chainage format
// (ch: 1+234.500) and the text cases (upper, lower). "{{" and "}}" are
// literal braces. A value with no format step is printed in its quantity's
// own format - a length to 3 decimals, an area to 1, a bearing in whole
// seconds - so "{distance}" is already a surveyor's distance.
//
// ABSENT IS NOT ZERO: a line of the template with a field whose value the
// target does not have (a point with no level, a line with no heights for
// its grade) is DROPPED, and a label whose every line is dropped is not
// drawn. "RL {z:.3f}" never prints "RL 0.000" for a point that has no RL.
//
// Numbers are rounded half away from zero and written without the locale,
// by the rules dimension_text.hpp states for a dimension's number, because a
// label is a number somebody signs as well.

#include <map>
#include <string>
#include <string_view>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/entity/annotation.hpp"

namespace katana::entity {

// What a value is, which says which steps apply to it and how it is printed
// with none.
enum class LabelQuantity {
    Text,     // printed as it is
    Integer,  // an id, a segment number
    Number,   // a plain number, 3 decimals
    Length,   // model units (metres), 3 decimals
    Area,     // square model units, 1 decimal
    Angle,    // radians, counter-clockwise; printed in DMS
    Bearing,  // radians clockwise from grid north, [0, 2 pi); printed in DMS
    Chainage, // model units along an alignment; printed as a plain 3-decimal number
};

struct LabelValue {
    LabelQuantity quantity = LabelQuantity::Number;
    double number = 0.0;
    std::string text{};

    [[nodiscard]] static LabelValue ofText(std::string text)
    {
        return LabelValue{LabelQuantity::Text, 0.0, std::move(text)};
    }
    [[nodiscard]] static LabelValue of(LabelQuantity quantity, double number)
    {
        return LabelValue{quantity, number, {}};
    }
};

// Name -> value. Ordered so that anything listing them lists them the same way.
using LabelValues = std::map<std::string, LabelValue, std::less<>>;

// The value names a label of `kind` can use, in the order HELP lists them.
// Every kind also has the common ones: id, layer, code, point (the point
// number property), description, and prop.NAME for any property.
[[nodiscard]] std::vector<std::string_view> labelValueNames(LabelKind kind);

// Checks a template for `kind`: braces balanced, every field naming a value
// the kind has (or a prop.NAME), every step known and applicable. A style is
// refused with the first problem found, named, rather than drawing a label
// with a hole in it.
[[nodiscard]] katana::core::Status checkLabelTemplate(std::string_view templateText,
                                                      LabelKind kind);

// The label's text: each template line with its fields filled, a line with
// an absent value dropped, the lines joined by '\n'. Empty when every line was
// dropped. A malformed template (which a stored style cannot have) prints its
// fields as "?" rather than failing a paint.
[[nodiscard]] std::string formatLabel(std::string_view templateText, const LabelValues& values);

// The formats the steps use, for the command line's replies and the tests.
//
// `decimals` fixed decimals, rounded half away from zero; "-0.000" is "0.000".
[[nodiscard]] std::string formatFixed(double value, int decimals);
// 123°45'06" (secondDecimals decimals on the seconds), the seconds rounded
// first and carried, so 59.9996" never prints as 60". A bearing is taken
// modulo a turn; an angle keeps its sign.
[[nodiscard]] std::string formatDms(double radians, int secondDecimals);
// 123°45' (whole minutes, rounded and carried).
[[nodiscard]] std::string formatDm(double radians);
// N 56°14'54" E: the quadrant bearing of a whole-circle bearing.
[[nodiscard]] std::string formatQuadrantBearing(double bearing, int secondDecimals);
// 1+234.500: kilometres, then metres to `decimals` padded to three whole
// digits; a negative chainage is -0+012.000.
[[nodiscard]] std::string formatChainage(double chainage, int decimals);

} // namespace katana::entity
