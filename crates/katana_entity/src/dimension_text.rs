//! Formatting a measured distance for a dimension label.
//!
//! This is a number that appears on a drawing somebody signs, so the rules
//! are pinned rather than left to whatever the platform's printf does.
//!
//! ORDER OF OPERATIONS is scale, then round, then fix the decimals - DIMLFAC,
//! DIMRND, DIMDEC in AutoCAD's names. Rounding before scaling rounds in the
//! wrong units and gives a different number.
//!
//! TIES GO AWAY FROM ZERO, which is what a surveyor expects and what AutoCAD
//! does: 2.5 at zero decimals is 3, not 2. The C++ rounds through
//! std::llround, which C17 7.12.9.6-7 specifies to round halfway cases away
//! from zero regardless of the rounding mode; f64::round is specified the
//! same way, and Rust has no rounding mode to change.
//!
//! THE OUTPUT IS LOCALE-INDEPENDENT: integer digits and hand-emitted
//! fraction digits, never a locale's decimal comma.
//!
//! UNITS ARE A SCALE FACTOR AND A SUFFIX, never a unit enum: the entity layer
//! cannot see the geodesy layer, and duplicating its units would be a second
//! definition of a foot.

use crate::tables::DimensionStyle;

/// Past this magnitude a double has no fractional part left to print, so the
/// value is emitted whole. 2^53 is where consecutive integers stop being
/// representable.
const NO_FRACTION_ABOVE: f64 = 9_007_199_254_740_992.0; // 2^53

/// std::llround: half away from zero. Every caller keeps |value| below 9e18,
/// inside i64.
fn llround(value: f64) -> i64 {
    value.round() as i64
}

fn whole_of(value: f64) -> String {
    // Beyond the range of a long long, the C++ falls back to to_chars on the
    // double with a fixed precision of zero: its exact digits, which {:.0}
    // writes too. The tie direction does not matter there: at that magnitude
    // the value has no fraction to break a tie with.
    if value.abs() < 9.0e18 {
        return format!("{}", value as i64);
    }
    format!("{value:.0}")
}

fn power_of_ten(exponent: i32) -> f64 {
    let mut value = 1.0;
    for _ in 0..exponent {
        value *= 10.0;
    }
    value
}

/// Strips trailing zeros, and then the decimal point if nothing is left after
/// it. "12.000" becomes "12", never "12."; "0.000" becomes "0", never "" - an
/// empty label draws nothing and reads as a rendering fault.
fn strip_trailing_zeros(text: &mut String) {
    if !text.contains('.') {
        return;
    }
    while text.ends_with('0') {
        text.pop();
    }
    if text.ends_with('.') {
        text.pop();
    }
    if text.is_empty() || text == "-" {
        *text = String::from("0");
    }
}

/// Formats `measurement` (model units, never negative) under `style`.
///
/// Never fails: a style that reached the model has been validated, and a
/// dimension label that could not be produced would leave a blank on a
/// drawing with nothing to say why.
pub fn format_measurement(measurement: f64, style: &DimensionStyle) -> String {
    if !measurement.is_finite() {
        // Cannot happen for a validated entity, but saying so beats printing
        // "inf" or "nan" on a drawing as though it were a length.
        return format!("{}?{}", style.prefix, style.suffix);
    }

    // 1. DIMLFAC: to display units.
    let scale = if style.unit_scale.is_finite() && style.unit_scale > 0.0 {
        style.unit_scale
    } else {
        1.0
    };
    let mut value = measurement * scale;

    // 2. DIMRND: to a multiple, ties away from zero.
    if style.round_to.is_finite() && style.round_to > 0.0 {
        let steps = value / style.round_to;
        if steps.abs() < 9.0e18 {
            value = llround(steps) as f64 * style.round_to;
        }
    }

    // 3. DIMDEC: fix the decimals.
    let decimals = style.decimals.clamp(0, 12);
    let negative = value < 0.0;
    let mut magnitude = value.abs();

    let mut text = if magnitude >= NO_FRACTION_ABOVE || decimals == 0 {
        // llround for the tie direction again, for the same reason.
        if magnitude < 9.0e18 {
            magnitude = llround(magnitude) as f64;
        }
        whole_of(magnitude)
    } else {
        // trunc and the subtraction are both exact for every finite double,
        // so the only rounding is the one below.
        let mut whole = magnitude.trunc();
        let fraction = magnitude - whole;
        let units = power_of_ten(decimals);

        let mut digits = llround(fraction * units);
        if digits as f64 >= units {
            // 0.9999 at two decimals rounds to 100, which is a carry into the
            // whole part rather than a third digit.
            digits = 0;
            whole += 1.0;
        }

        let mut text = whole_of(whole);
        text.push('.');
        // Zero padded on the LEFT: 0.05 at two decimals is "05", not "5".
        text.push_str(&format!("{digits:0>width$}", width = decimals as usize));
        text
    };

    if style.suppress_trailing_zeros {
        strip_trailing_zeros(&mut text);
    }
    // The sign goes on after the zero suppression, so "-0.000" becomes "0" and
    // never "-0" - a negative zero on a drawing is a distraction at best.
    if negative && text != "0" {
        text.insert(0, '-');
    }
    format!("{}{}{}", style.prefix, text, style.suffix)
}

/// The same, for a dimension that carries its own text. An override wins
/// VERBATIM: no prefix, no suffix, no rounding and no unit scale, because DXF
/// does not apply DIMPOST to overridden text and a surveyor who typed a value
/// means that value.
pub fn dimension_label(measurement: f64, text_override: &str, style: &DimensionStyle) -> String {
    if !text_override.is_empty() {
        return text_override.to_owned();
    }
    format_measurement(measurement, style)
}
