//! Angle units, DMS text and bearing conversions.
//!
//! Conventions
//!   * Every angle is stored and computed in radians (IEEE-754 binary64).
//!   * An azimuth is measured clockwise from grid north and lives in [0, 2*pi).
//!   * A quadrant bearing is the acute angle from the north or south meridian
//!     towards east or west, e.g. "S 12°30'00\" W"; its angle lives in [0, pi/2].
//!   * DMS text is parsed and formatted through integer arc-second arithmetic so
//!     that rounding never produces a field of 60 minutes or 60 seconds.

#![allow(
    clippy::neg_cmp_op_on_partial_ord,
    reason = "Preserve the C++ oracle's negated floating-point comparisons, including NaN rejection."
)]

use katana_core::text::{format_exact_real, parse_finite_double};
use katana_core::{Error, ErrorCode, Result};
use katana_math::numerics::{DEG_TO_RAD, HALF_PI, PI, RAD_TO_DEG, TWO_PI, normalize_angle};

pub const GON_TO_RAD: f64 = PI / 200.0;
pub const RAD_TO_GON: f64 = 200.0 / PI;
pub const ARC_SECOND_TO_RAD: f64 = PI / 648000.0;
pub const RAD_TO_ARC_SECOND: f64 = 648000.0 / PI;

pub const fn degrees_to_radians(degrees: f64) -> f64 {
    degrees * DEG_TO_RAD
}

pub const fn radians_to_degrees(radians: f64) -> f64 {
    radians * RAD_TO_DEG
}

pub const fn gon_to_radians(gon: f64) -> f64 {
    gon * GON_TO_RAD
}

pub const fn radians_to_gon(radians: f64) -> f64 {
    radians * RAD_TO_GON
}

pub const fn arc_seconds_to_radians(arc_seconds: f64) -> f64 {
    arc_seconds * ARC_SECOND_TO_RAD
}

pub const fn radians_to_arc_seconds(radians: f64) -> f64 {
    radians * RAD_TO_ARC_SECOND
}

/// Wraps any finite angle into the azimuth range [0, 2*pi).
pub fn normalize_azimuth(radians: f64) -> f64 {
    normalize_angle(radians)
}

/// Sexagesimal angle. The sign is kept apart from the fields so that values in
/// (-1°, 0°) such as -0°30' are representable.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DmsAngle {
    pub negative: bool,
    pub degrees: u32,
    /// 0..59
    pub minutes: u32,
    /// [0, 60)
    pub seconds: f64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum BearingMeridian {
    #[default]
    North,
    South,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum BearingSide {
    #[default]
    East,
    West,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct QuadrantBearing {
    pub meridian: BearingMeridian,
    /// Radians, [0, pi/2].
    pub angle: f64,
    pub side: BearingSide,
}

// Largest magnitude below which every integer is exactly representable (2^53).
// Not a tolerance: it bounds the integer arc-second arithmetic of the formatter.
const MAX_EXACT_INTEGER: f64 = 9007199254740992.0;

const MAX_SECONDS_DECIMALS: i32 = 9;

const fn is_blank(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r')
}

fn trimmed(mut text: &[u8]) -> &[u8] {
    while let [first, rest @ ..] = text
        && is_blank(*first)
    {
        text = rest;
    }
    while let [rest @ .., last] = text
        && is_blank(*last)
    {
        text = rest;
    }
    text
}

#[derive(Clone, Copy, Default)]
struct NumberToken {
    value: f64,
    has_fraction: bool,
}

// Cursor over the text of one sexagesimal angle.
struct Scanner<'a> {
    text: &'a [u8],
    position: usize,
}

impl<'a> Scanner<'a> {
    fn new(text: &'a [u8]) -> Self {
        Self { text, position: 0 }
    }

    fn at_end(&self) -> bool {
        self.position >= self.text.len()
    }

    fn skip_blanks(&mut self) {
        while !self.at_end() && is_blank(self.text[self.position]) {
            self.position += 1;
        }
    }

    fn consume(&mut self, token: &[u8]) -> bool {
        if !self.text[self.position..].starts_with(token) {
            return false;
        }
        self.position += token.len();
        true
    }

    // Unsigned decimal number without exponent: digits[.digits] or .digits.
    // A general number parser alone would also accept "1e5", "inf" and "nan".
    fn number(&mut self) -> Option<NumberToken> {
        let begin = self.position;
        let mut end = begin;
        let mut digit_count = 0;
        let mut has_fraction = false;
        while end < self.text.len() && self.text[end].is_ascii_digit() {
            end += 1;
            digit_count += 1;
        }
        if end < self.text.len() && self.text[end] == b'.' {
            has_fraction = true;
            end += 1;
            while end < self.text.len() && self.text[end].is_ascii_digit() {
                end += 1;
                digit_count += 1;
            }
        }
        if digit_count == 0 {
            return None;
        }
        // parse_finite_double is the port of the std::from_chars the C++ calls
        // here: it refuses an overflow and an underflow to zero as that does.
        let value = parse_finite_double(&self.text[begin..end])?;
        self.position = end;
        Some(NumberToken {
            value,
            has_fraction,
        })
    }
}

// Field marks in UTF-8. Longer alternatives first ("''" before "'").
const DEGREE_MARKS: [&[u8]; 6] = [b"\xC2\xB0", b"\xC2\xBA", b"d", b"D", b":", b"-"];
const MINUTE_MARKS: [&[u8]; 6] = [b"\xE2\x80\xB2", b"'", b"m", b"M", b":", b"-"];
const SECOND_MARKS: [&[u8]; 5] = [b"\xE2\x80\xB3", b"\"", b"''", b"s", b"S"];

fn consume_one_mark(scanner: &mut Scanner<'_>, marks: &[&[u8]]) {
    scanner.skip_blanks();
    for mark in marks {
        if scanner.consume(mark) {
            break;
        }
    }
    scanner.skip_blanks();
}

fn parse_failure(text: &str, message: &str) -> Error {
    Error::new(ErrorCode::ParseFailure, message).with_context(text)
}

// Parses an unsigned sexagesimal angle into arc-seconds.
fn parse_unsigned_arc_seconds(text: &[u8], original: &str) -> Result<f64> {
    let mut scanner = Scanner::new(text);
    let mut fields = [NumberToken::default(); 3];
    let mut field_count = 0;

    while field_count < fields.len() {
        scanner.skip_blanks();
        let Some(token) = scanner.number() else {
            break;
        };
        fields[field_count] = token;
        if field_count == 0 {
            consume_one_mark(&mut scanner, &DEGREE_MARKS);
        } else if field_count == 1 {
            consume_one_mark(&mut scanner, &MINUTE_MARKS);
        } else {
            consume_one_mark(&mut scanner, &SECOND_MARKS);
        }
        field_count += 1;
    }

    if field_count == 0 {
        return Err(parse_failure(
            original,
            "expected an angle such as 45°30'15\"",
        ));
    }
    if !scanner.at_end() {
        return Err(parse_failure(original, "unexpected characters in angle"));
    }
    if fields[..field_count - 1]
        .iter()
        .any(|field| field.has_fraction)
    {
        return Err(parse_failure(
            original,
            "only the last angle field may carry a fraction",
        ));
    }
    if field_count >= 2 && !(fields[1].value < 60.0) {
        return Err(parse_failure(original, "minutes must be below 60"));
    }
    if field_count == 3 && !(fields[2].value < 60.0) {
        return Err(parse_failure(original, "seconds must be below 60"));
    }
    Ok(fields[0].value * 3600.0 + fields[1].value * 60.0 + fields[2].value)
}

fn format_fields(dms: &DmsAngle, seconds_decimals: i32) -> String {
    // printf's "%0*.*f": the seconds zero-padded to "SS" plus the decimals.
    let precision = seconds_decimals as usize;
    let width = if seconds_decimals > 0 {
        precision + 3
    } else {
        2
    };
    let seconds = format!("{:0width$.precision$}", dms.seconds);

    let mut text = String::new();
    if dms.negative {
        text.push('-');
    }
    text.push_str(&format!("{}", dms.degrees));
    text.push('\u{B0}');
    if dms.minutes < 10 {
        text.push('0');
    }
    text.push_str(&format!("{}", dms.minutes));
    text.push('\'');
    text.push_str(&seconds);
    text.push('"');
    text
}

/// InvalidArgument when minutes >= 60, seconds outside [0, 60) or not finite.
pub fn dms_to_radians(dms: DmsAngle) -> Result<f64> {
    if dms.minutes >= 60 {
        return Err(
            Error::new(ErrorCode::InvalidArgument, "minutes must be below 60")
                .with_context(format!("minutes={}", dms.minutes)),
        );
    }
    if !dms.seconds.is_finite() || dms.seconds < 0.0 || !(dms.seconds < 60.0) {
        return Err(
            Error::new(ErrorCode::InvalidArgument, "seconds must lie in [0, 60)")
                .with_context(format!("seconds={}", format_exact_real(dms.seconds))),
        );
    }
    let arc_seconds = f64::from(dms.degrees) * 3600.0 + f64::from(dms.minutes) * 60.0 + dms.seconds;
    let radians = arc_seconds * ARC_SECOND_TO_RAD;
    Ok(if dms.negative { -radians } else { radians })
}

/// Splits an angle into fields with the seconds rounded to `seconds_decimals`
/// (0..9) decimals; the rounding carries into minutes and degrees.
/// InvalidArgument for a non-finite angle, decimals out of range, or an angle
/// so large that the requested resolution is not representable in a double.
pub fn radians_to_dms(radians: f64, seconds_decimals: i32) -> Result<DmsAngle> {
    if !radians.is_finite() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "angle is not finite",
        ));
    }
    if !(0..=MAX_SECONDS_DECIMALS).contains(&seconds_decimals) {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "seconds decimals must lie in 0..9",
        )
        .with_context(format!("secondsDecimals={seconds_decimals}")));
    }
    let mut units_per_second: i64 = 1;
    for _ in 0..seconds_decimals {
        units_per_second *= 10;
    }
    let total_units = radians.abs() * RAD_TO_ARC_SECOND * units_per_second as f64;
    if !(total_units < MAX_EXACT_INTEGER) {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "angle is too large for the requested seconds resolution",
        ));
    }

    // Integer arithmetic from here on: the carry of a rounded 59.999... seconds
    // into minutes and degrees is exact. std::llround rounds halves away from
    // zero, as f64::round does; the guard above keeps the cast in range.
    let units = total_units.round() as i64;
    let units_per_minute = 60 * units_per_second;
    let units_per_degree = 60 * units_per_minute;
    let degrees = units / units_per_degree;
    let remainder = units % units_per_degree;
    if degrees > i64::from(u32::MAX) {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "angle is too large to split into DMS",
        ));
    }

    Ok(DmsAngle {
        // never "-0°00'00""
        negative: radians < 0.0 && units != 0,
        degrees: degrees as u32,
        minutes: (remainder / units_per_minute) as u32,
        seconds: (remainder % units_per_minute) as f64 / units_per_second as f64,
    })
}

/// Parses "[+-]D[ M[ S]]" into radians. Accepted field marks: degrees `°`, `º`,
/// `d`, `:`, `-`; minutes `'`, `′`, `m`, `:`, `-`; seconds `"`, `″`, `''`, `s`;
/// blanks may replace or surround any mark. Only the last field may carry a
/// fraction ("45.5", "45°30.25'", "45 30 15.5"). ParseFailure otherwise.
pub fn parse_dms(text: &str) -> Result<f64> {
    let mut body = trimmed(text.as_bytes());
    let mut negative = false;
    if let [sign @ (b'+' | b'-'), rest @ ..] = body {
        negative = *sign == b'-';
        body = rest;
    }
    let arc_seconds = parse_unsigned_arc_seconds(body, text)?;
    let radians = arc_seconds * ARC_SECOND_TO_RAD;
    Ok(if negative { -radians } else { radians })
}

/// Formats as `[-]D°MM'SS.ss"` (UTF-8 degree sign).
pub fn format_dms(radians: f64, seconds_decimals: i32) -> Result<String> {
    let dms = radians_to_dms(radians, seconds_decimals)?;
    Ok(format_fields(&dms, seconds_decimals))
}

/// Quadrant choice on the axes: azimuth 0 -> N 0 E, pi/2 -> N 90 E, pi -> S 0 E,
/// 3*pi/2 -> S 90 W. InvalidArgument for a non-finite azimuth.
pub fn azimuth_to_bearing(azimuth: f64) -> Result<QuadrantBearing> {
    if !azimuth.is_finite() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "azimuth is not finite",
        ));
    }
    let wrapped = normalize_azimuth(azimuth);
    if wrapped <= HALF_PI {
        return Ok(QuadrantBearing {
            meridian: BearingMeridian::North,
            angle: wrapped,
            side: BearingSide::East,
        });
    }
    if wrapped <= PI {
        return Ok(QuadrantBearing {
            meridian: BearingMeridian::South,
            angle: PI - wrapped,
            side: BearingSide::East,
        });
    }
    if wrapped <= PI + HALF_PI {
        return Ok(QuadrantBearing {
            meridian: BearingMeridian::South,
            angle: wrapped - PI,
            side: BearingSide::West,
        });
    }
    Ok(QuadrantBearing {
        meridian: BearingMeridian::North,
        angle: TWO_PI - wrapped,
        side: BearingSide::West,
    })
}

/// InvalidArgument when the angle is outside [0, pi/2] or not finite.
pub fn bearing_to_azimuth(bearing: QuadrantBearing) -> Result<f64> {
    // A non-finite angle is refused first, so the range test sees finite values
    // only and agrees with the C++ "angle < 0 || angle > pi/2".
    if !bearing.angle.is_finite() || !(0.0..=HALF_PI).contains(&bearing.angle) {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "bearing angle must lie in [0, pi/2]",
        )
        .with_context(format!("angle={}", format_exact_real(bearing.angle))));
    }
    let north = bearing.meridian == BearingMeridian::North;
    let east = bearing.side == BearingSide::East;
    let azimuth = if north {
        if east {
            bearing.angle
        } else {
            TWO_PI - bearing.angle
        }
    } else if east {
        PI - bearing.angle
    } else {
        PI + bearing.angle
    };
    // N 0 W is azimuth 0, not 2*pi
    Ok(normalize_azimuth(azimuth))
}

/// Parses "N 45°30'15\" E" (any parse_dms angle format between the letters,
/// case insensitive) into an azimuth. ParseFailure on malformed text or an
/// angle outside [0°, 90°].
pub fn parse_bearing(text: &str) -> Result<f64> {
    let body = trimmed(text.as_bytes());
    if body.len() < 3 {
        return Err(parse_failure(
            text,
            "expected a bearing such as N 45°30'15\" E",
        ));
    }
    let first = body[0];
    let last = body[body.len() - 1];

    let meridian = match first {
        b'N' | b'n' => BearingMeridian::North,
        b'S' | b's' => BearingMeridian::South,
        _ => return Err(parse_failure(text, "bearing must start with N or S")),
    };
    let side = match last {
        b'E' | b'e' => BearingSide::East,
        b'W' | b'w' => BearingSide::West,
        _ => return Err(parse_failure(text, "bearing must end with E or W")),
    };

    let angle_text = trimmed(&body[1..body.len() - 1]);
    let arc_seconds = parse_unsigned_arc_seconds(angle_text, text)?;
    // Compared in arc-seconds so that exactly 90° is accepted despite the
    // rounding of the conversion to radians.
    if arc_seconds > 90.0 * 3600.0 {
        return Err(parse_failure(
            text,
            "bearing angle must not exceed 90 degrees",
        ));
    }
    // std::min(a, b) is (b < a) ? b : a.
    let converted = arc_seconds * ARC_SECOND_TO_RAD;
    let angle = if HALF_PI < converted {
        HALF_PI
    } else {
        converted
    };
    bearing_to_azimuth(QuadrantBearing {
        meridian,
        angle,
        side,
    })
}

/// Formats an azimuth as `N 45°30'15" E`.
pub fn format_bearing(azimuth: f64, seconds_decimals: i32) -> Result<String> {
    let bearing = azimuth_to_bearing(azimuth)?;
    let angle = format_dms(bearing.angle, seconds_decimals)?;
    let mut text = String::from(if bearing.meridian == BearingMeridian::North {
        "N "
    } else {
        "S "
    });
    text.push_str(&angle);
    text.push_str(if bearing.side == BearingSide::East {
        " E"
    } else {
        " W"
    });
    Ok(text)
}
