#include "katana/entity/dimension_text.hpp"

#include <charconv>
#include <cmath>
#include <limits>

#include "katana/entity/tables.hpp"

namespace katana::entity {

namespace {

// Past this magnitude a double has no fractional part left to print, so the
// value is emitted whole. 2^53 is where consecutive integers stop being
// representable; anything at or beyond it is already an integer.
constexpr double kNoFractionAbove = 9007199254740992.0; // 2^53

// std::to_chars only: locale independent by specification, where printf and
// iostreams would emit "1,5" under a European locale and quietly change what
// the drawing says.
[[nodiscard]] std::string digitsOf(long long value)
{
    char buffer[32];
    const auto [end, error] = std::to_chars(buffer, buffer + sizeof(buffer), value);
    if (error != std::errc{}) {
        return "0"; // unreachable for any long long; a blank label would be worse
    }
    return std::string(buffer, end);
}

[[nodiscard]] std::string wholeOf(double value)
{
    // Beyond the range of long long, fall back to to_chars on the double with a
    // fixed precision of zero. The tie direction does not matter there: at that
    // magnitude the value has no fraction to break a tie with.
    if (std::abs(value) < 9.0e18) {
        return digitsOf(static_cast<long long>(value));
    }
    char buffer[64];
    const auto [end, error] =
        std::to_chars(buffer, buffer + sizeof(buffer), value, std::chars_format::fixed, 0);
    return error == std::errc{} ? std::string(buffer, end) : std::string("0");
}

[[nodiscard]] double powerOfTen(int exponent)
{
    double value = 1.0;
    for (int i = 0; i < exponent; ++i) {
        value *= 10.0;
    }
    return value;
}

// Strips trailing zeros, and then the decimal point if nothing is left after
// it. "12.000" becomes "12", never "12."; "0.000" becomes "0", never "" - an
// empty label draws nothing and reads as a rendering fault rather than as the
// number zero.
void stripTrailingZeros(std::string& text)
{
    if (text.find('.') == std::string::npos) {
        return;
    }
    while (!text.empty() && text.back() == '0') {
        text.pop_back();
    }
    if (!text.empty() && text.back() == '.') {
        text.pop_back();
    }
    if (text.empty() || text == "-") {
        text = "0";
    }
}

} // namespace

std::string formatMeasurement(double measurement, const DimensionStyle& style)
{
    if (!std::isfinite(measurement)) {
        // Cannot happen for a validated entity - the model rejects non-finite
        // geometry - but a label is not the place to discover it. Saying so
        // beats printing "inf" or "nan" on a drawing as though it were a length.
        return style.prefix + "?" + style.suffix;
    }

    // 1. DIMLFAC: to display units.
    const double scale =
        std::isfinite(style.unitScale) && style.unitScale > 0.0 ? style.unitScale : 1.0;
    double value = measurement * scale;

    // 2. DIMRND: to a multiple, ties away from zero.
    if (std::isfinite(style.roundTo) && style.roundTo > 0.0) {
        const double steps = value / style.roundTo;
        if (std::abs(steps) < 9.0e18) {
            // std::llround, not std::round through a cast: C17 7.12.9.6-7 pins
            // llround to round halfway cases away from zero whatever the
            // current rounding mode is, which is what a surveyor expects and
            // what the platform's printf does NOT do.
            value = static_cast<double>(std::llround(steps)) * style.roundTo;
        }
    }

    // 3. DIMDEC: fix the decimals.
    const int decimals = std::clamp(style.decimals, 0, 12);
    const bool negative = value < 0.0;
    double magnitude = std::abs(value);

    std::string text;
    if (magnitude >= kNoFractionAbove || decimals == 0) {
        // llround for the tie direction again, for the same reason.
        if (magnitude < 9.0e18) {
            magnitude = static_cast<double>(std::llround(magnitude));
        }
        text = wholeOf(magnitude);
    } else {
        // trunc and the subtraction are both exact for every finite double, so
        // the only rounding is the one below.
        double whole = std::trunc(magnitude);
        const double fraction = magnitude - whole;
        const double units = powerOfTen(decimals);

        long long digits = std::llround(fraction * units);
        if (static_cast<double>(digits) >= units) {
            // 0.9999 at two decimals rounds to 100, which is a carry into the
            // whole part rather than a third digit.
            digits = 0;
            whole += 1.0;
        }

        text = wholeOf(whole);
        text.push_back('.');
        std::string fractionText = digitsOf(digits);
        // Zero padded on the LEFT: 0.05 at two decimals is "05", not "5".
        if (fractionText.size() < static_cast<std::size_t>(decimals)) {
            fractionText.insert(0, static_cast<std::size_t>(decimals) - fractionText.size(), '0');
        }
        text += fractionText;
    }

    if (style.suppressTrailingZeros) {
        stripTrailingZeros(text);
    }
    // The sign goes on after the zero suppression, so "-0.000" becomes "0" and
    // never "-0" - a negative zero on a drawing is a distraction at best.
    if (negative && text != "0") {
        text.insert(0, 1, '-');
    }
    return style.prefix + text + style.suffix;
}

std::string dimensionLabel(double measurement, const std::string& textOverride,
                           const DimensionStyle& style)
{
    if (!textOverride.empty()) {
        // Verbatim. DXF does not apply DIMPOST to overridden text, and someone
        // who typed a value means that value - not that value scaled, rounded
        // and wrapped in a prefix they did not ask for.
        return textOverride;
    }
    return formatMeasurement(measurement, style);
}

} // namespace katana::entity
