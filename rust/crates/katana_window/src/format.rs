//! Numbers as the desktop application shows them to a person. One definition, so the command log,
//! the panels and the GIS dialogs group digits alike.

use katana_geometry::Point2;

/// A coordinate, a length or an angle as the status bar and the Properties panel show it: four
/// places, in the C locale, so a value copied from the panel types back on the command line.
/// `QString::number(value, 'f', 4)`.
pub fn plan_number(value: f64) -> String {
    qt_fixed(value, 4)
}

/// "x, y" at four places: the cursor's readout and a vertex in the panel.
pub fn plan_point(point: &Point2) -> String {
    format!("{}, {}", plan_number(point.x), plan_number(point.y))
}

/// Thousands grouped in the user's locale: "2,000,000" - or "2000000" where the locale is C,
/// which groups nothing.
///
/// The C++ is `QLocale().toString(value)`, the system locale. Qt's C locale omits the group
/// separator (`QLocale::OmitGroupSeparator`, which Qt sets on C by default), and that is the
/// locale the headless tests run in under LANG=C; every other locale groups. The port has no
/// locale database: a locale that is not C groups in threes with a comma, as the English locales
/// do. A locale with another separator (a stop, a thin space) is not reproduced - recorded under
/// "Not done" (docs/rust.md).
pub fn grouped(value: u64) -> String {
    let digits = format!("{value}");
    if !locale_groups_digits() {
        return digits;
    }
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (at, digit) in digits.chars().enumerate() {
        if at != 0 && (digits.len() - at).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

/// Whether the system locale groups thousands: every locale but C (and POSIX, its other name).
/// On Unix the locale numbers are written in is LC_ALL, else LC_NUMERIC, else LANG, as the C
/// library and Qt read them; unset is C.
fn locale_groups_digits() -> bool {
    #[cfg(windows)]
    {
        // Windows has no C locale for a person's session: the user's locale always groups.
        true
    }
    #[cfg(not(windows))]
    {
        let locale = ["LC_ALL", "LC_NUMERIC", "LANG"]
            .iter()
            .filter_map(|name| std::env::var(name).ok())
            .find(|value| !value.is_empty())
            .unwrap_or_default();
        let language = locale.split(['.', '@']).next().unwrap_or("");
        !(language.is_empty() || language == "C" || language == "POSIX")
    }
}

/// `QString::number(value, 'f', decimals)`: `decimals` places, the C locale's digits, "nan",
/// "inf" and "-inf" for what is not finite.
///
/// Qt formats through double-conversion's fixed mode, which rounds an exact half AWAY from zero
/// (12.40625 at four places is 12.4063); Rust's formatting rounds it to even (12.4062), as C's
/// printf does. They differ only on an exact tie - a value whose exact binary expansion ends in a
/// 5 at the place after the last kept - so the tie is found exactly and rounded Qt's way.
pub(crate) fn qt_fixed(value: f64, decimals: usize) -> String {
    if value.is_nan() {
        return String::from("nan");
    }
    if value.is_infinite() {
        return String::from(if value < 0.0 { "-inf" } else { "inf" });
    }
    let to_even = format!("{value:.decimals$}");
    let longer = format!("{value:.places$}", places = decimals + 1);
    if !longer.ends_with('5') || !is_exact_decimal(value, &longer, decimals + 1) {
        return to_even;
    }
    // The exact value is `longer`, a half at the last kept place: away from zero, by adding one
    // at that place to the magnitude.
    let mut digits: Vec<u8> = longer[..longer.len() - 1].bytes().collect();
    if digits.last() == Some(&b'.') {
        digits.pop();
    }
    let mut at = digits.len();
    loop {
        while at > 0 && digits[at - 1] == b'.' {
            at -= 1;
        }
        if at == 0 || digits[at - 1] == b'-' {
            // Every digit carried (9.5 to 10): a new leading one, after any sign.
            digits.insert(at, b'1');
            break;
        }
        at -= 1;
        if digits[at] == b'9' {
            digits[at] = b'0';
            continue;
        }
        digits[at] += 1;
        break;
    }
    // Only ASCII digits, a point and a sign were written.
    String::from_utf8(digits).unwrap_or(to_even)
}

/// Whether `text` (`value` written with `places` decimals) is `value` exactly: it reads back as
/// `value`, and its decimal m / 10^places is a dyadic rational - 5^places divides m - so it is a
/// double itself, the one nearest to it, `value`.
fn is_exact_decimal(value: f64, text: &str, places: usize) -> bool {
    if text.parse::<f64>().ok() != Some(value) {
        return false;
    }
    let mut decimal: u128 = 0;
    for byte in text.bytes().filter(u8::is_ascii_digit) {
        let Some(next) = decimal
            .checked_mul(10)
            .and_then(|scaled| scaled.checked_add(u128::from(byte - b'0')))
        else {
            return false;
        };
        decimal = next;
    }
    let Some(five_power) = u32::try_from(places)
        .ok()
        .and_then(|places| 5_u128.checked_pow(places))
    else {
        return false;
    };
    decimal.is_multiple_of(five_power)
}

#[cfg(test)]
mod tests {
    use super::qt_fixed;

    // Worked by hand: 12.40625 is 397/32 exactly, a half at the fifth place, which Qt's fixed
    // mode rounds away from zero and Rust's to even; 12.40624 is no tie and both round it down.
    #[test]
    fn an_exact_half_rounds_away_from_zero_as_qt_rounds_it() {
        assert_eq!(qt_fixed(12.40625, 4), "12.4063");
        assert_eq!(qt_fixed(-12.40625, 4), "-12.4063");
        assert_eq!(qt_fixed(12.40624, 4), "12.4062");
        assert_eq!(qt_fixed(0.125, 2), "0.13");
        assert_eq!(qt_fixed(-0.125, 2), "-0.13");
        assert_eq!(qt_fixed(2.5, 0), "3");
        assert_eq!(qt_fixed(0.5, 0), "1");
        assert_eq!(qt_fixed(-0.5, 0), "-1");
        assert_eq!(qt_fixed(9.5, 0), "10");
    }

    #[test]
    fn what_is_not_finite_is_spelt_as_qt_spells_it() {
        assert_eq!(qt_fixed(f64::NAN, 4), "nan");
        assert_eq!(qt_fixed(f64::INFINITY, 4), "inf");
        assert_eq!(qt_fixed(f64::NEG_INFINITY, 4), "-inf");
    }
}
