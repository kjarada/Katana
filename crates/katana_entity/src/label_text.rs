//! The label template language (docs/annotation.md, "Templates").
//!
//! A label style's text is literal text with fields in braces:
//!
//! ```text
//! {bearing:dms} {distance:.3f}
//! {area:m2:.1f} m2\n{area:ha:.4f} ha
//! RL {z:.3f}
//! ```
//!
//! A field is a VALUE name and then any number of ':'-separated steps applied
//! left to right: unit conversions (m, mm, km, ft; m2, ha, km2, ac; deg, rad,
//! gon), a number format (.Nf, N decimals; +.Nf the same with a plus before a
//! value above nothing), an angle format (dms, dms.N, dm, qb for a quadrant
//! bearing), the chainage format (ch: 1+234.500) and the text cases (upper,
//! lower). "{{" and "}}" are literal braces. A value with no format step is
//! printed in its quantity's own format.
//!
//! ABSENT IS NOT ZERO: a line of the template with a field whose value the
//! target does not have is DROPPED, and a label whose every line is dropped is
//! not drawn. "RL {z:.3f}" never prints "RL 0.000" for a point that has no RL.
//!
//! Numbers are rounded half away from zero and written without the locale,
//! by the rules dimension_text.rs states for a dimension's number.

use std::collections::BTreeMap;

use katana_core::error::{ErrorCode, Status, make_error};
use katana_math::numerics::{PI, RAD_TO_DEG, TWO_PI, normalize_angle};

use crate::annotation::LabelKind;
use crate::dimension_text::format_measurement;
use crate::entity::to_string;
use crate::tables::DimensionStyle;
use crate::text_block::text_lines;

/// What a value is, which says which steps apply to it and how it is printed
/// with none.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum LabelQuantity {
    /// Printed as it is.
    Text,
    /// An id, a segment number.
    Integer,
    /// A plain number, 3 decimals.
    #[default]
    Number,
    /// Model units (metres), 3 decimals.
    Length,
    /// Square model units, 1 decimal.
    Area,
    /// Radians, counter-clockwise; printed in DMS.
    Angle,
    /// Radians clockwise from grid north, [0, 2 pi); printed in DMS.
    Bearing,
    /// Model units along an alignment; printed as a plain 3-decimal number.
    Chainage,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LabelValue {
    pub quantity: LabelQuantity,
    pub number: f64,
    pub text: String,
}

impl LabelValue {
    pub fn of_text(text: impl Into<String>) -> LabelValue {
        LabelValue {
            quantity: LabelQuantity::Text,
            number: 0.0,
            text: text.into(),
        }
    }

    pub fn of(quantity: LabelQuantity, number: f64) -> LabelValue {
        LabelValue {
            quantity,
            number,
            text: String::new(),
        }
    }
}

/// Name -> value. Ordered so that anything listing them lists them the same
/// way.
pub type LabelValues = BTreeMap<String, LabelValue>;

// The value names by kind. The common ones come first for every kind.
const COMMON_VALUES: [&str; 5] = ["id", "layer", "code", "point", "description"];
const POINT_VALUES: [&str; 6] = ["x", "y", "easting", "northing", "z", "rl"];
const SEGMENT_VALUES: [&str; 8] = [
    "bearing", "distance", "length", "dx", "dy", "dz", "grade", "segment",
];
const ARC_VALUES: [&str; 6] = ["radius", "length", "chord", "delta", "bearing", "tangent"];
const AREA_VALUES: [&str; 4] = ["area", "perimeter", "x", "y"];
const CHAINAGE_VALUES: [&str; 4] = ["chainage", "x", "y", "alignment"];

/// A square metre per acre: the international acre is exactly 4046.8564224
/// m2 (66 x 660 international feet of 0.3048 m).
const SQUARE_METRES_PER_ACRE: f64 = 4046.8564224;
/// A foot is exactly 0.3048 m (the international yard and pound agreement of
/// 1959).
const METRES_PER_FOOT: f64 = 0.3048;

fn is_property_name(name: &str) -> bool {
    name.len() > 5 && name.starts_with("prop.")
}

/// One field's parts: the value name and its steps.
struct Field<'a> {
    name: &'a str,
    steps: Vec<&'a str>,
}

fn parse_field(inside: &str) -> Field<'_> {
    let mut parts = inside.split(':');
    let name = parts.next().unwrap_or("");
    Field {
        name,
        steps: parts.collect(),
    }
}

/// std::from_chars for an int: decimal digits with an optional '-', the whole
/// text consumed (the callers require that), and nothing that overflows.
fn from_chars_int(digits: &str) -> Option<i32> {
    let body = digits.strip_prefix('-').unwrap_or(digits);
    if body.is_empty() || !body.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

/// A number format step: ".3f" is 3 decimals; "+.3f" the same with the sign
/// always written, so a height ABOVE something reads "+0.250" beside one
/// below it reading "-0.250".
struct FixedFormat {
    decimals: i32,
    plus: bool,
}

/// None when `step` is not a number format.
fn fixed_format(step: &str) -> Option<FixedFormat> {
    let (plus, step) = match step.strip_prefix('+') {
        Some(rest) => (true, rest),
        None => (false, step),
    };
    if step.len() < 3 || !step.starts_with('.') || !step.ends_with('f') {
        return None;
    }
    let decimals = from_chars_int(&step[1..step.len() - 1])?;
    if !(0..=12).contains(&decimals) {
        return None;
    }
    Some(FixedFormat { decimals, plus })
}

/// `number` to the format's decimals; with "+", a value that is more than
/// nothing at those decimals gets its plus. Nothing gets no sign at all.
fn format_fixed_step(number: f64, format: &FixedFormat) -> String {
    let mut text = format_fixed(number, format.decimals);
    if format.plus
        && !text.is_empty()
        && !text.starts_with('-')
        && text.bytes().any(|byte| byte != b'0' && byte != b'.')
    {
        text.insert(0, '+');
    }
    text
}

/// "dms" -> 0, "dms.2" -> 2; None when it is not a DMS step.
fn dms_decimals(step: &str, stem: &str) -> Option<i32> {
    if step == stem {
        return Some(0);
    }
    if step.len() <= stem.len() + 1
        || !step.as_bytes().starts_with(stem.as_bytes())
        || step.as_bytes()[stem.len()] != b'.'
    {
        return None;
    }
    let decimals = from_chars_int(&step[stem.len() + 1..])?;
    if !(0..=6).contains(&decimals) {
        return None;
    }
    Some(decimals)
}

fn is_angular(quantity: LabelQuantity) -> bool {
    quantity == LabelQuantity::Angle || quantity == LabelQuantity::Bearing
}

/// Applies the steps to a value: the text, or the reason a step does not
/// apply.
fn apply(value: &LabelValue, steps: &[&str]) -> Result<String, String> {
    if value.quantity == LabelQuantity::Text {
        let mut text = value.text.clone();
        for &step in steps {
            if step == "upper" {
                text = text.to_ascii_uppercase();
            } else if step == "lower" {
                text = text.to_ascii_lowercase();
            } else {
                return Err(format!("'{step}' does not apply to text"));
            }
        }
        return Ok(text);
    }

    let mut number = value.number;
    let quantity = value.quantity;
    let bearing_wrapped = |radians: f64| {
        if quantity == LabelQuantity::Bearing {
            normalize_angle(radians)
        } else {
            radians
        }
    };
    // Once an angle has been turned into a plain number of degrees (deg, gon,
    // rad) it is printed as a number.
    let mut angle_as_number = false;
    for (index, &step) in steps.iter().enumerate() {
        let last = index + 1 == steps.len();
        if let Some(format) = fixed_format(step) {
            if !last {
                return Err(String::from("a number format must be the last step"));
            }
            if is_angular(quantity) && !angle_as_number {
                return Err(String::from(
                    "an angle needs deg, rad or gon before a number format",
                ));
            }
            return Ok(format_fixed_step(number, &format));
        }
        if is_angular(quantity) && !angle_as_number {
            if let Some(decimals) = dms_decimals(step, "dms") {
                if !last {
                    return Err(String::from("dms must be the last step"));
                }
                return Ok(format_dms(bearing_wrapped(number), decimals));
            }
            if step == "dm" {
                if !last {
                    return Err(String::from("dm must be the last step"));
                }
                return Ok(format_dm(bearing_wrapped(number)));
            }
            if let Some(decimals) = dms_decimals(step, "qb") {
                if quantity != LabelQuantity::Bearing {
                    return Err(String::from("qb applies to a bearing only"));
                }
                if !last {
                    return Err(String::from("qb must be the last step"));
                }
                return Ok(format_quadrant_bearing(number, decimals));
            }
            if step == "deg" {
                number = bearing_wrapped(number) * RAD_TO_DEG;
                angle_as_number = true;
                continue;
            }
            if step == "gon" {
                number = bearing_wrapped(number) * 200.0 / PI;
                angle_as_number = true;
                continue;
            }
            if step == "rad" {
                angle_as_number = true;
                continue;
            }
            return Err(format!("'{step}' does not apply to an angle"));
        }
        if quantity == LabelQuantity::Length
            || quantity == LabelQuantity::Number
            || quantity == LabelQuantity::Chainage
        {
            match step {
                "m" => continue,
                "mm" => {
                    number *= 1000.0;
                    continue;
                }
                "km" => {
                    number /= 1000.0;
                    continue;
                }
                "ft" => {
                    number /= METRES_PER_FOOT;
                    continue;
                }
                _ => {}
            }
        }
        if quantity == LabelQuantity::Chainage
            && let Some(decimals) = dms_decimals(step, "ch")
        {
            if !last {
                return Err(String::from("ch must be the last step"));
            }
            return Ok(format_chainage(
                number,
                if decimals == 0 { 3 } else { decimals },
            ));
        }
        if quantity == LabelQuantity::Area {
            match step {
                "m2" => continue,
                "ha" => {
                    number /= 10000.0;
                    continue;
                }
                "km2" => {
                    number /= 1.0e6;
                    continue;
                }
                "ac" => {
                    number /= SQUARE_METRES_PER_ACRE;
                    continue;
                }
                _ => {}
            }
        }
        return Err(format!("unknown or inapplicable step '{step}'"));
    }

    // No format step: the quantity's own format.
    Ok(match quantity {
        LabelQuantity::Integer => format_fixed(number, 0),
        LabelQuantity::Area => format_fixed(number, 1),
        LabelQuantity::Angle | LabelQuantity::Bearing => {
            if angle_as_number {
                format_fixed(number, 4)
            } else {
                format_dms(bearing_wrapped(number), 0)
            }
        }
        LabelQuantity::Text
        | LabelQuantity::Number
        | LabelQuantity::Length
        | LabelQuantity::Chainage => format_fixed(number, 3),
    })
}

/// The quantity a value name has for checking a template, with no target to
/// ask.
fn quantity_of(name: &str) -> LabelQuantity {
    match name {
        "id" | "segment" => LabelQuantity::Integer,
        "layer" | "code" | "point" | "description" | "alignment" => LabelQuantity::Text,
        "bearing" => LabelQuantity::Bearing,
        "delta" => LabelQuantity::Angle,
        "area" => LabelQuantity::Area,
        "chainage" => LabelQuantity::Chainage,
        "grade" | "dz" | "z" | "rl" => LabelQuantity::Number,
        _ => LabelQuantity::Length,
    }
}

fn kind_has(kind: LabelKind, name: &str) -> bool {
    COMMON_VALUES.contains(&name) || label_value_names(kind).contains(&name)
}

/// A piece of a template line: literal text, or the inside of a field.
enum Piece<'a> {
    Text(&'a str),
    Field(&'a str),
}

/// Walks a template line: literal text and fields, calling `on` for each.
/// False when the braces do not balance.
fn walk<'a>(line: &'a str, mut on: impl FnMut(Piece<'a>)) -> bool {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let character = bytes[i];
        if character == b'{' {
            if i + 1 < bytes.len() && bytes[i + 1] == b'{' {
                on(Piece::Text("{"));
                i += 2;
                continue;
            }
            let Some(close) = line[i + 1..].find('}').map(|at| at + i + 1) else {
                return false;
            };
            let inside = &line[i + 1..close];
            if inside.contains('{') {
                return false;
            }
            on(Piece::Field(inside));
            i = close + 1;
            continue;
        }
        if character == b'}' {
            if i + 1 < bytes.len() && bytes[i + 1] == b'}' {
                on(Piece::Text("}"));
                i += 2;
                continue;
            }
            return false;
        }
        let end = line[i..].find(['{', '}']).map_or(bytes.len(), |at| at + i);
        on(Piece::Text(&line[i..end]));
        i = end;
    }
    true
}

/// The value names a label of `kind` can use, in the order HELP lists them.
/// Every kind also has the common ones: id, layer, code, point (the point
/// number property), description, and prop.NAME for any property.
pub fn label_value_names(kind: LabelKind) -> Vec<&'static str> {
    match kind {
        LabelKind::Point => POINT_VALUES.to_vec(),
        LabelKind::Segment => SEGMENT_VALUES.to_vec(),
        LabelKind::Arc => ARC_VALUES.to_vec(),
        LabelKind::Area => AREA_VALUES.to_vec(),
        LabelKind::Chainage => CHAINAGE_VALUES.to_vec(),
    }
}

/// The same check as check_label_template against any set of values - a
/// label kind's, a leader's (leader_values.rs): `quantity_of_name` says what a
/// value name is, None for a name there is no such value, and the refusal of
/// such a name is worded by `unknown_value`. A prop.NAME is never asked about:
/// a property may be a number or text, so only a step that applies to neither
/// is refused.
pub fn check_template(
    template_text: &str,
    quantity_of_name: &dyn Fn(&str) -> Option<LabelQuantity>,
    unknown_value: &dyn Fn(&str) -> String,
) -> Status {
    let mut problem = String::new();
    for line in text_lines(template_text) {
        let balanced = walk(line, |piece| {
            let Piece::Field(inside) = piece else {
                return;
            };
            if !problem.is_empty() {
                return;
            }
            let field = parse_field(inside);
            if field.name.is_empty() {
                problem = String::from("an empty field {}");
                return;
            }
            if is_property_name(field.name) {
                // A property may be a number or text: refuse only steps that
                // apply to neither.
                let as_number = apply(&LabelValue::of(LabelQuantity::Number, 0.0), &field.steps);
                let as_text = apply(&LabelValue::of_text(""), &field.steps);
                if let (Err(why), Err(_)) = (as_number, as_text) {
                    problem = format!("{{{inside}}}: {why}");
                }
                return;
            }
            let Some(quantity) = quantity_of_name(field.name) else {
                problem = unknown_value(field.name);
                return;
            };
            if let Err(why) = apply(&LabelValue::of(quantity, 0.0), &field.steps) {
                problem = format!("{{{inside}}}: {why}");
            }
        });
        if !balanced {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "a label template's braces do not balance (use {{ and }} for a brace)",
                line,
            ));
        }
        if !problem.is_empty() {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "the label template is not valid",
                problem,
            ));
        }
    }
    Ok(())
}

/// Checks a template for `kind`: braces balanced, every field naming a value
/// the kind has (or a prop.NAME), every step known and applicable. A style is
/// refused with the first problem found, named, rather than drawing a label
/// with a hole in it.
pub fn check_label_template(template_text: &str, kind: LabelKind) -> Status {
    check_template(
        template_text,
        &|name: &str| {
            if kind_has(kind, name) {
                Some(quantity_of(name))
            } else {
                None
            }
        },
        &|name: &str| format!("a {} label has no value '{}'", to_string(kind), name),
    )
}

/// The value names a template's fields name, in the order they first appear,
/// each once: what a refusal says a target lacks. A template whose braces do
/// not balance names none.
pub fn template_fields(template_text: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for line in text_lines(template_text) {
        let mut found = Vec::new();
        let balanced = walk(line, |piece| {
            if let Piece::Field(inside) = piece {
                found.push(parse_field(inside).name);
            }
        });
        if !balanced {
            return Vec::new();
        }
        for name in found {
            if !name.is_empty() && !names.iter().any(|known| known == name) {
                names.push(name.to_owned());
            }
        }
    }
    names
}

/// `template_text` with each field's value name passed through `rename`,
/// which answers the name to write in its place or None to keep it; the
/// field's steps, the literal text, the line breaks and the doubled braces
/// are kept as they were. A template whose braces do not balance is returned
/// as it is: it names nothing, so there is nothing to follow.
pub fn rename_template_fields(
    template_text: &str,
    rename: &dyn Fn(&str) -> Option<String>,
) -> String {
    let bytes = template_text.as_bytes();
    let mut result = String::with_capacity(template_text.len());
    let mut i = 0;
    while i < bytes.len() {
        let character = bytes[i];
        if character == b'{' && i + 1 < bytes.len() && bytes[i + 1] == b'{' {
            result.push_str("{{");
            i += 2;
        } else if character == b'}' && i + 1 < bytes.len() && bytes[i + 1] == b'}' {
            result.push_str("}}");
            i += 2;
        } else if character == b'}' {
            return template_text.to_owned(); // a lone '}': not balanced
        } else if character == b'{' {
            // A field is one line's: the same bound walk() puts on it.
            let Some(close) = template_text[i + 1..].find('}').map(|at| at + i + 1) else {
                return template_text.to_owned();
            };
            let inside = &template_text[i + 1..close];
            if inside.contains(['{', '\n']) {
                return template_text.to_owned();
            }
            let field = parse_field(inside);
            result.push('{');
            match rename(field.name) {
                Some(renamed) => {
                    result.push_str(&renamed);
                    result.push_str(&inside[field.name.len()..]);
                }
                None => result.push_str(inside),
            }
            result.push('}');
            i = close + 1;
        } else {
            // Up to the next brace, which is always a character boundary.
            let end = template_text[i..]
                .find(['{', '}'])
                .map_or(bytes.len(), |at| at + i);
            result.push_str(&template_text[i..end]);
            i = end;
        }
    }
    result
}

/// The label's text: each template line with its fields filled, a line with
/// an absent value dropped, the lines joined by '\n'. Empty when every line
/// was dropped. A malformed template (which a stored style cannot have)
/// prints its fields as "?" rather than failing a paint.
pub fn format_label(template_text: &str, values: &LabelValues) -> String {
    let mut result = String::new();
    let mut any_line = false;
    for line in text_lines(template_text) {
        let mut text = String::new();
        let mut absent = false;
        let balanced = walk(line, |piece| match piece {
            Piece::Text(literal) => text.push_str(literal),
            Piece::Field(inside) => {
                let field = parse_field(inside);
                let Some(found) = values.get(field.name) else {
                    absent = true;
                    return;
                };
                match apply(found, &field.steps) {
                    Ok(formatted) => text.push_str(&formatted),
                    Err(_) => text.push('?'),
                }
            }
        });
        if !balanced {
            text = line.to_owned();
        }
        if absent {
            continue;
        }
        if any_line {
            result.push('\n');
        }
        result.push_str(&text);
        any_line = true;
    }
    result
}

/// One value in its quantity's own format, as a field with no step prints
/// it: what LEADER VALUES lists beside each name.
pub fn format_value(value: &LabelValue) -> String {
    apply(value, &[]).unwrap_or_else(|_| String::from("?"))
}

/// `decimals` fixed decimals, rounded half away from zero; "-0.000" is
/// "0.000". (Not katana_core's format_fixed, which rounds ties to even.)
pub fn format_fixed(value: f64, decimals: i32) -> String {
    let style = DimensionStyle {
        decimals: decimals.clamp(0, 12),
        ..DimensionStyle::default()
    };
    let mut text = format_measurement(value, &style);
    // format_measurement keeps the sign of a value that rounds to zero at
    // these decimals ("-0.000"); a label never says minus nothing.
    if text.starts_with('-') && text.bytes().all(|byte| matches!(byte, b'-' | b'0' | b'.')) {
        text.remove(0);
    }
    text
}

/// std::llround for the magnitudes an angle in seconds reaches. The C++ has
/// no guard for a value past a long long (undefined there; it saturates
/// here).
fn llround(value: f64) -> i64 {
    value.round() as i64
}

/// 123°45'06" (second_decimals decimals on the seconds), the seconds rounded
/// first and carried, so 59.9996" never prints as 60". A bearing is taken
/// modulo a turn by the caller; an angle keeps its sign.
pub fn format_dms(radians: f64, second_decimals: i32) -> String {
    if !radians.is_finite() {
        return String::from("?");
    }
    let decimals = second_decimals.clamp(0, 6);
    let negative = radians < 0.0;
    // Rounded ONCE, in units of the last printed digit of a second, then
    // split: rounding the seconds on their own printed 59.9996" as 60".
    let mut units_per_second = 1.0;
    for _ in 0..decimals {
        units_per_second *= 10.0;
    }
    let total_units = radians.abs() * RAD_TO_DEG * 3600.0 * units_per_second;
    let units = llround(total_units);
    let per_minute = (60.0 * units_per_second) as i64;
    let per_degree = 60 * per_minute;
    let degrees = units / per_degree;
    let minutes = (units % per_degree) / per_minute;
    let second_units = units % per_minute;

    let mut text = String::from(if negative && units != 0 { "-" } else { "" });
    text.push_str(&format!("{degrees}°{minutes:02}'"));
    let whole_seconds = second_units / units_per_second as i64;
    text.push_str(&format!("{whole_seconds:02}"));
    if decimals > 0 {
        let fraction = second_units % units_per_second as i64;
        text.push_str(&format!(".{fraction:0>width$}", width = decimals as usize));
    }
    text.push('"');
    text
}

/// 123°45' (whole minutes, rounded and carried).
pub fn format_dm(radians: f64) -> String {
    if !radians.is_finite() {
        return String::from("?");
    }
    let negative = radians < 0.0;
    let minutes_total = llround(radians.abs() * RAD_TO_DEG * 60.0);
    let mut text = String::from(if negative && minutes_total != 0 {
        "-"
    } else {
        ""
    });
    text.push_str(&format!(
        "{}°{:02}'",
        minutes_total / 60,
        minutes_total % 60
    ));
    text
}

/// N 56°14'54" E: the quadrant bearing of a whole-circle bearing.
pub fn format_quadrant_bearing(bearing: f64, second_decimals: i32) -> String {
    if !bearing.is_finite() {
        return String::from("?");
    }
    let b = normalize_angle(bearing);
    let quarter = 0.5 * PI;
    // N x E for [0, 90), S x E for [90, 180), S x W for [180, 270), N x W.
    if b < quarter {
        return format!("N {} E", format_dms(b, second_decimals));
    }
    if b < PI {
        return format!("S {} E", format_dms(PI - b, second_decimals));
    }
    if b < 3.0 * quarter {
        return format!("S {} W", format_dms(b - PI, second_decimals));
    }
    format!("N {} W", format_dms(TWO_PI - b, second_decimals))
}

/// 1+234.500: kilometres, then metres to `decimals` padded to three whole
/// digits; a negative chainage is -0+012.000.
pub fn format_chainage(chainage: f64, decimals: i32) -> String {
    if !chainage.is_finite() {
        return String::from("?");
    }
    // Rounded first and split after, so 999.9996 at three decimals is
    // 1+000.000 and not 0+1000.000.
    let fixed = format_fixed(chainage.abs(), decimals);
    let (whole, fraction) = match fixed.find('.') {
        Some(point) => (&fixed[..point], &fixed[point..]),
        None => (fixed.as_str(), ""),
    };
    let metres = if whole.len() > 3 {
        &whole[whole.len() - 3..]
    } else {
        whole
    };
    let kilometres = if whole.len() > 3 {
        &whole[..whole.len() - 3]
    } else {
        "0"
    };
    let negative = chainage < 0.0 && fixed.bytes().any(|byte| byte != b'0' && byte != b'.');
    format!(
        "{}{}+{:0>3}{}",
        if negative { "-" } else { "" },
        kilometres,
        metres,
        fraction
    )
}
