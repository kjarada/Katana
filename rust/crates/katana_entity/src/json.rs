//! nlohmann::json as serialization.cpp uses it: serde_json parses the text,
//! and this module gives the rest the C++ library's behaviour - its `dump()`
//! text, its reading rules (`get<T>`, `at`, `value`, a range-for) and the
//! `what()` text of the exceptions those throw. Private: no serde_json type
//! is in the crate's public interface.
//!
//! The C++ uses the plain `nlohmann::json`, whose objects are std::maps: the
//! keys come out SORTED, and an object's values are walked in key order. The
//! workspace turns serde_json's preserve_order on (other layers need
//! nlohmann's ordered_json), so both orders are made here explicitly rather
//! than left to serde_json's map.
//!
//! A number is written as nlohmann writes it: its own Grisu2 (Loitsch, PLDI
//! 2010), printed like printf's %g with at least one fractional digit ("1.0",
//! "1e+20", "1e-05"). Grisu2 is not always the shortest round-trip text, so
//! neither ryu nor katana_core's format_exact_real can stand in for it.

use serde_json::{Map, Number, Value};

/// A nlohmann exception, as its `what()` reads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct JsonError(pub(crate) String);

pub(crate) type JsonResult<T> = std::result::Result<T, JsonError>;

fn exception(kind: &str, id: u32, message: &str) -> JsonError {
    // nlohmann's exception::name, with JSON_DIAGNOSTICS off (the default the
    // build uses): no JSON pointer in the text.
    JsonError(format!("[json.exception.{kind}.{id}] {message}"))
}

/// other_error 501, which serialization.cpp throws for its own refusals.
pub(crate) fn other_error(message: &str) -> JsonError {
    exception("other_error", 501, message)
}

fn type_error(id: u32, message: &str) -> JsonError {
    exception("type_error", id, message)
}

/// nlohmann's `type_name()`.
fn type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// A number as nlohmann holds it: unsigned for a non-negative integer, signed
/// for a negative one, a double otherwise - the classification serde_json's
/// parser makes too.
#[derive(Clone, Copy)]
enum Numeric {
    Unsigned(u64),
    Signed(i64),
    Float(f64),
}

fn numeric(number: &Number) -> Numeric {
    if let Some(unsigned) = number.as_u64() {
        Numeric::Unsigned(unsigned)
    } else if let Some(signed) = number.as_i64() {
        Numeric::Signed(signed)
    } else {
        Numeric::Float(number.as_f64().unwrap_or(f64::NAN))
    }
}

fn number_or_type_error(value: &Value) -> JsonResult<Numeric> {
    match value {
        Value::Number(number) => Ok(numeric(number)),
        _ => Err(type_error(
            302,
            &format!("type must be number, but is {}", type_name(value)),
        )),
    }
}

// ---- get<T>() ---------------------------------------------------------------------
//
// nlohmann converts any number to an arithmetic type with a static_cast. The
// casts here are Rust's `as`, which agree with the C++ for every integer
// conversion (modular) and every in-range double; a double out of an
// integer's range is undefined in C++ and saturates here (docs/rust.md "Not
// done"). The types that are not one of the json's own number types (int,
// std::uint32_t, std::int32_t) also take a boolean, as nlohmann's from_json
// for them does; double, std::uint64_t and std::int64_t do not.

pub(crate) fn get_f64(value: &Value) -> JsonResult<f64> {
    Ok(match number_or_type_error(value)? {
        Numeric::Unsigned(unsigned) => unsigned as f64,
        Numeric::Signed(signed) => signed as f64,
        Numeric::Float(float) => float,
    })
}

pub(crate) fn get_u64(value: &Value) -> JsonResult<u64> {
    Ok(match number_or_type_error(value)? {
        Numeric::Unsigned(unsigned) => unsigned,
        Numeric::Signed(signed) => signed as u64,
        Numeric::Float(float) => float as u64,
    })
}

pub(crate) fn get_i64(value: &Value) -> JsonResult<i64> {
    Ok(match number_or_type_error(value)? {
        Numeric::Unsigned(unsigned) => unsigned as i64,
        Numeric::Signed(signed) => signed,
        Numeric::Float(float) => float as i64,
    })
}

fn number_or_boolean(value: &Value) -> JsonResult<Numeric> {
    match value {
        Value::Bool(flag) => Ok(Numeric::Unsigned(u64::from(*flag))),
        Value::Number(number) => Ok(numeric(number)),
        _ => Err(type_error(
            302,
            &format!("type must be number, but is {}", type_name(value)),
        )),
    }
}

pub(crate) fn get_i32(value: &Value) -> JsonResult<i32> {
    Ok(match number_or_boolean(value)? {
        Numeric::Unsigned(unsigned) => unsigned as i32,
        Numeric::Signed(signed) => signed as i32,
        Numeric::Float(float) => float as i32,
    })
}

pub(crate) fn get_u32(value: &Value) -> JsonResult<u32> {
    Ok(match number_or_boolean(value)? {
        Numeric::Unsigned(unsigned) => unsigned as u32,
        Numeric::Signed(signed) => signed as u32,
        Numeric::Float(float) => float as u32,
    })
}

pub(crate) fn get_bool(value: &Value) -> JsonResult<bool> {
    match value {
        Value::Bool(flag) => Ok(*flag),
        _ => Err(type_error(
            302,
            &format!("type must be boolean, but is {}", type_name(value)),
        )),
    }
}

pub(crate) fn get_string(value: &Value) -> JsonResult<String> {
    match value {
        Value::String(text) => Ok(text.clone()),
        _ => Err(type_error(
            302,
            &format!("type must be string, but is {}", type_name(value)),
        )),
    }
}

/// get<std::vector<double>>().
pub(crate) fn get_f64_vec(value: &Value) -> JsonResult<Vec<f64>> {
    match value {
        Value::Array(items) => items.iter().map(get_f64).collect(),
        _ => Err(type_error(
            302,
            &format!("type must be array, but is {}", type_name(value)),
        )),
    }
}

// ---- at, contains, value, size, iteration -----------------------------------------

/// `j.at(key)` on an object.
pub(crate) fn at<'a>(value: &'a Value, key: &str) -> JsonResult<&'a Value> {
    match value {
        Value::Object(map) => map
            .get(key)
            .ok_or_else(|| exception("out_of_range", 403, &format!("key '{key}' not found"))),
        _ => Err(type_error(
            304,
            &format!("cannot use at() with {}", type_name(value)),
        )),
    }
}

/// `j.at(index)` on an array.
pub(crate) fn at_index(value: &Value, index: usize) -> JsonResult<&Value> {
    match value {
        Value::Array(items) => items.get(index).ok_or_else(|| {
            exception(
                "out_of_range",
                401,
                &format!("array index {index} is out of range"),
            )
        }),
        _ => Err(type_error(
            304,
            &format!("cannot use at() with {}", type_name(value)),
        )),
    }
}

/// `j.contains(key)`: false for anything that is not an object.
pub(crate) fn contains(value: &Value, key: &str) -> bool {
    matches!(value, Value::Object(map) if map.contains_key(key))
}

/// `j.value(key, fallback)`: the member read with `get` when it is there,
/// the fallback when it is not, and type_error 306 on anything that is not an
/// object.
pub(crate) fn value_or<T>(
    value: &Value,
    key: &str,
    fallback: T,
    get: impl FnOnce(&Value) -> JsonResult<T>,
) -> JsonResult<T> {
    match value {
        Value::Object(map) => match map.get(key) {
            Some(member) => get(member),
            None => Ok(fallback),
        },
        _ => Err(type_error(
            306,
            &format!("cannot use value() with {}", type_name(value)),
        )),
    }
}

/// `j.size()`: an array's or an object's members, 0 for null, 1 for anything
/// else.
pub(crate) fn size(value: &Value) -> usize {
    match value {
        Value::Null => 0,
        Value::Array(items) => items.len(),
        Value::Object(map) => map.len(),
        _ => 1,
    }
}

/// What `for (const Json& item : j)` visits: an array's elements, an
/// object's values in key order, nothing for null, and a primitive itself.
pub(crate) fn items(value: &Value) -> Vec<&Value> {
    match value {
        Value::Null => Vec::new(),
        Value::Array(elements) => elements.iter().collect(),
        Value::Object(map) => sorted_entries(map)
            .into_iter()
            .map(|(_, member)| member)
            .collect(),
        _ => vec![value],
    }
}

/// An object's members in key order, as a std::map holds them.
pub(crate) fn sorted_entries(map: &Map<String, Value>) -> Vec<(&String, &Value)> {
    let mut entries: Vec<(&String, &Value)> = map.iter().collect();
    entries.sort_by(|first, second| first.0.cmp(second.0));
    entries
}

// ---- making values ----------------------------------------------------------------

/// A double as nlohmann stores it; a non-finite one is dumped as null, which
/// is what serde_json's Number refuses to hold.
pub(crate) fn real(value: f64) -> Value {
    Number::from_f64(value).map_or(Value::Null, Value::Number)
}

pub(crate) fn reals(values: &[f64]) -> Value {
    Value::Array(values.iter().map(|&value| real(value)).collect())
}

// ---- parse and dump ---------------------------------------------------------------

/// `Json::parse(text)`: strict, the whole text one value. nlohmann skips a
/// UTF-8 byte order mark at the start, so this does too. Its parse_error's
/// what() text is not reproduced (docs/rust.md "Not done"): the error carries
/// serde_json's description of the fault instead.
pub(crate) fn parse(text: &str) -> JsonResult<Value> {
    let text = text.strip_prefix('\u{FEFF}').unwrap_or(text);
    serde_json::from_str(text)
        .map_err(|error| exception("parse_error", 101, &format!("parse error: {error}")))
}

/// `j.dump()`: compact, keys sorted, nlohmann's number and string text.
pub(crate) fn dump(value: &Value) -> String {
    let mut out = String::new();
    write_value(&mut out, value);
    out
}

fn write_value(out: &mut String, value: &Value) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(flag) => out.push_str(if *flag { "true" } else { "false" }),
        Value::Number(number) => match numeric(number) {
            Numeric::Unsigned(unsigned) => out.push_str(&format!("{unsigned}")),
            Numeric::Signed(signed) => out.push_str(&format!("{signed}")),
            Numeric::Float(float) => out.push_str(&dump_float(float)),
        },
        Value::String(text) => write_string(out, text),
        Value::Array(elements) => {
            out.push('[');
            for (index, element) in elements.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_value(out, element);
            }
            out.push(']');
        }
        Value::Object(map) => {
            out.push('{');
            for (index, (key, member)) in sorted_entries(map).into_iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_string(out, key);
                out.push(':');
                write_value(out, member);
            }
            out.push('}');
        }
    }
}

/// nlohmann's dump_escaped with ensure_ascii off: the two-character escapes,
/// \u00XX (lower-case hex) for the other control characters, and everything
/// else - DEL and every non-ASCII character included - as it is.
fn write_string(out: &mut String, text: &str) {
    out.push('"');
    for character in text.chars() {
        match character {
            '\u{08}' => out.push_str("\\b"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\u{0C}' => out.push_str("\\f"),
            '\r' => out.push_str("\\r"),
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            control if u32::from(control) <= 0x1F => {
                out.push_str(&format!("\\u{:04x}", u32::from(control)));
            }
            other => out.push(other),
        }
    }
    out.push('"');
}

/// nlohmann's dump_float: null for a non-finite value, else its to_chars.
fn dump_float(value: f64) -> String {
    if !value.is_finite() {
        return String::from("null");
    }
    to_chars(value)
}

// ---- nlohmann's dtoa: Grisu2 (detail/conversions/to_chars.hpp) ----------------------
//
// Ported line by line from nlohmann 3.12.0, whose code is Florian Loitsch's
// reference implementation, slightly modified (MIT). The unsigned arithmetic
// wraps as the C++'s does; the algorithm keeps every product in range.

#[derive(Clone, Copy)]
struct DiyFp {
    f: u64,
    e: i32,
}

impl DiyFp {
    const fn new(f: u64, e: i32) -> Self {
        Self { f, e }
    }

    /// x - y, for x.e == y.e and x.f >= y.f.
    fn sub(x: DiyFp, y: DiyFp) -> DiyFp {
        DiyFp::new(x.f.wrapping_sub(y.f), x.e)
    }

    /// x * y, rounded: only the upper 64 bits are returned.
    fn mul(x: DiyFp, y: DiyFp) -> DiyFp {
        let u_lo = x.f & 0xFFFF_FFFF;
        let u_hi = x.f >> 32;
        let v_lo = y.f & 0xFFFF_FFFF;
        let v_hi = y.f >> 32;

        let p0 = u_lo.wrapping_mul(v_lo);
        let p1 = u_lo.wrapping_mul(v_hi);
        let p2 = u_hi.wrapping_mul(v_lo);
        let p3 = u_hi.wrapping_mul(v_hi);

        let p0_hi = p0 >> 32;
        let p1_lo = p1 & 0xFFFF_FFFF;
        let p1_hi = p1 >> 32;
        let p2_lo = p2 & 0xFFFF_FFFF;
        let p2_hi = p2 >> 32;

        let mut q = p0_hi.wrapping_add(p1_lo).wrapping_add(p2_lo);
        // Round, ties up: only the highest bit of the low half is needed.
        q = q.wrapping_add(1_u64 << (64 - 32 - 1));
        let h = p3
            .wrapping_add(p2_hi)
            .wrapping_add(p1_hi)
            .wrapping_add(q >> 32);
        DiyFp::new(h, x.e + y.e + 64)
    }

    /// Normalised so that the significand is >= 2^63; x.f != 0.
    fn normalize(mut x: DiyFp) -> DiyFp {
        while (x.f >> 63) == 0 {
            x.f <<= 1;
            x.e -= 1;
        }
        x
    }

    /// Normalised to the exponent `target_exponent`.
    fn normalize_to(x: DiyFp, target_exponent: i32) -> DiyFp {
        let delta = x.e - target_exponent;
        DiyFp::new(x.f << delta, target_exponent)
    }
}

struct Boundaries {
    w: DiyFp,
    minus: DiyFp,
    plus: DiyFp,
}

/// The normalised diyfp of `value` (finite and positive) and its boundaries
/// m- and m+.
fn compute_boundaries(value: f64) -> Boundaries {
    const PRECISION: i32 = 53; // p, the hidden bit included
    const BIAS: i32 = 1024 - 1 + (PRECISION - 1);
    const MIN_EXP: i32 = 1 - BIAS;
    const HIDDEN_BIT: u64 = 1 << (PRECISION - 1);

    let bits = value.to_bits();
    let exponent = bits >> (PRECISION - 1);
    let fraction = bits & (HIDDEN_BIT - 1);

    let is_denormal = exponent == 0;
    let v = if is_denormal {
        DiyFp::new(fraction, MIN_EXP)
    } else {
        DiyFp::new(fraction + HIDDEN_BIT, exponent as i32 - BIAS)
    };

    // m- = (v- + v) / 2 and m+ = (v + v+) / 2: all real numbers strictly
    // between them round to v.
    let lower_boundary_is_closer = fraction == 0 && exponent > 1;
    let m_plus = DiyFp::new(2 * v.f + 1, v.e - 1);
    let m_minus = if lower_boundary_is_closer {
        DiyFp::new(4 * v.f - 1, v.e - 2)
    } else {
        DiyFp::new(2 * v.f - 1, v.e - 1)
    };

    let w_plus = DiyFp::normalize(m_plus);
    let w_minus = DiyFp::normalize_to(m_minus, w_plus.e);
    Boundaries {
        w: DiyFp::normalize(v),
        minus: w_minus,
        plus: w_plus,
    }
}

const ALPHA: i32 = -60;
const GAMMA: i32 = -32;
// nlohmann's static_asserts on the two: the digit generation needs the
// exponent of the scaled value in [-60, -32].
const _: () = assert!(ALPHA >= -60 && GAMMA <= -32);

#[derive(Clone, Copy)]
struct CachedPower {
    f: u64,
    e: i32,
    k: i32,
}

const fn cached(f: u64, e: i32, k: i32) -> CachedPower {
    CachedPower { f, e, k }
}

/// The normalised powers of ten 10^-300 ... 10^324 in steps of 8, from the
/// reference implementation.
const CACHED_POWERS: [CachedPower; 79] = [
    cached(0xAB70FE17C79AC6CA, -1060, -300),
    cached(0xFF77B1FCBEBCDC4F, -1034, -292),
    cached(0xBE5691EF416BD60C, -1007, -284),
    cached(0x8DD01FAD907FFC3C, -980, -276),
    cached(0xD3515C2831559A83, -954, -268),
    cached(0x9D71AC8FADA6C9B5, -927, -260),
    cached(0xEA9C227723EE8BCB, -901, -252),
    cached(0xAECC49914078536D, -874, -244),
    cached(0x823C12795DB6CE57, -847, -236),
    cached(0xC21094364DFB5637, -821, -228),
    cached(0x9096EA6F3848984F, -794, -220),
    cached(0xD77485CB25823AC7, -768, -212),
    cached(0xA086CFCD97BF97F4, -741, -204),
    cached(0xEF340A98172AACE5, -715, -196),
    cached(0xB23867FB2A35B28E, -688, -188),
    cached(0x84C8D4DFD2C63F3B, -661, -180),
    cached(0xC5DD44271AD3CDBA, -635, -172),
    cached(0x936B9FCEBB25C996, -608, -164),
    cached(0xDBAC6C247D62A584, -582, -156),
    cached(0xA3AB66580D5FDAF6, -555, -148),
    cached(0xF3E2F893DEC3F126, -529, -140),
    cached(0xB5B5ADA8AAFF80B8, -502, -132),
    cached(0x87625F056C7C4A8B, -475, -124),
    cached(0xC9BCFF6034C13053, -449, -116),
    cached(0x964E858C91BA2655, -422, -108),
    cached(0xDFF9772470297EBD, -396, -100),
    cached(0xA6DFBD9FB8E5B88F, -369, -92),
    cached(0xF8A95FCF88747D94, -343, -84),
    cached(0xB94470938FA89BCF, -316, -76),
    cached(0x8A08F0F8BF0F156B, -289, -68),
    cached(0xCDB02555653131B6, -263, -60),
    cached(0x993FE2C6D07B7FAC, -236, -52),
    cached(0xE45C10C42A2B3B06, -210, -44),
    cached(0xAA242499697392D3, -183, -36),
    cached(0xFD87B5F28300CA0E, -157, -28),
    cached(0xBCE5086492111AEB, -130, -20),
    cached(0x8CBCCC096F5088CC, -103, -12),
    cached(0xD1B71758E219652C, -77, -4),
    cached(0x9C40000000000000, -50, 4),
    cached(0xE8D4A51000000000, -24, 12),
    cached(0xAD78EBC5AC620000, 3, 20),
    cached(0x813F3978F8940984, 30, 28),
    cached(0xC097CE7BC90715B3, 56, 36),
    cached(0x8F7E32CE7BEA5C70, 83, 44),
    cached(0xD5D238A4ABE98068, 109, 52),
    cached(0x9F4F2726179A2245, 136, 60),
    cached(0xED63A231D4C4FB27, 162, 68),
    cached(0xB0DE65388CC8ADA8, 189, 76),
    cached(0x83C7088E1AAB65DB, 216, 84),
    cached(0xC45D1DF942711D9A, 242, 92),
    cached(0x924D692CA61BE758, 269, 100),
    cached(0xDA01EE641A708DEA, 295, 108),
    cached(0xA26DA3999AEF774A, 322, 116),
    cached(0xF209787BB47D6B85, 348, 124),
    cached(0xB454E4A179DD1877, 375, 132),
    cached(0x865B86925B9BC5C2, 402, 140),
    cached(0xC83553C5C8965D3D, 428, 148),
    cached(0x952AB45CFA97A0B3, 455, 156),
    cached(0xDE469FBD99A05FE3, 481, 164),
    cached(0xA59BC234DB398C25, 508, 172),
    cached(0xF6C69A72A3989F5C, 534, 180),
    cached(0xB7DCBF5354E9BECE, 561, 188),
    cached(0x88FCF317F22241E2, 588, 196),
    cached(0xCC20CE9BD35C78A5, 614, 204),
    cached(0x98165AF37B2153DF, 641, 212),
    cached(0xE2A0B5DC971F303A, 667, 220),
    cached(0xA8D9D1535CE3B396, 694, 228),
    cached(0xFB9B7CD9A4A7443C, 720, 236),
    cached(0xBB764C4CA7A44410, 747, 244),
    cached(0x8BAB8EEFB6409C1A, 774, 252),
    cached(0xD01FEF10A657842C, 800, 260),
    cached(0x9B10A4E5E9913129, 827, 268),
    cached(0xE7109BFBA19C0C9D, 853, 276),
    cached(0xAC2820D9623BF429, 880, 284),
    cached(0x80444B5E7AA7CF85, 907, 292),
    cached(0xBF21E44003ACDD2D, 933, 300),
    cached(0x8E679C2F5E44FF8F, 960, 308),
    cached(0xD433179D9C8CB841, 986, 316),
    cached(0x9E19DB92B4E31BA9, 1013, 324),
];

/// The cached power c such that the exponent of w * c lies in [ALPHA, GAMMA]
/// for a normalised w of exponent `e`.
fn cached_power_for_binary_exponent(e: i32) -> CachedPower {
    const CACHED_POWERS_MIN_DEC_EXP: i32 = -300;
    const CACHED_POWERS_DEC_STEP: i32 = 8;
    // The same k as ceil((ALPHA - e - 1) * log10(2)) for |e| <= 1500, without
    // floating point: log10(2) ~= 78913 / 2^18. Integer division truncates
    // towards zero in both languages.
    let f = ALPHA - e - 1;
    let k = (f * 78913) / (1 << 18) + i32::from(f > 0);
    let index =
        (-CACHED_POWERS_MIN_DEC_EXP + k + (CACHED_POWERS_DEC_STEP - 1)) / CACHED_POWERS_DEC_STEP;
    CACHED_POWERS[index as usize]
}

/// For n != 0, k such that 10^(k-1) <= n < 10^k, and 10^(k-1).
fn find_largest_pow10(n: u32) -> (i32, u32) {
    let mut k = 10;
    let mut pow10 = 1_000_000_000;
    while k > 1 && n < pow10 {
        k -= 1;
        pow10 /= 10;
    }
    (k, pow10)
}

/// Decrements the last digit while that takes the decimal closer to w. The
/// tests are written in this order to avoid overflow in unsigned arithmetic.
fn grisu2_round(buffer: &mut [u8], dist: u64, delta: u64, mut rest: u64, ten_k: u64) {
    let last = buffer.len() - 1;
    while rest < dist
        && delta.wrapping_sub(rest) >= ten_k
        && (rest.wrapping_add(ten_k) < dist
            || dist.wrapping_sub(rest) > rest.wrapping_add(ten_k).wrapping_sub(dist))
    {
        buffer[last] -= 1;
        rest = rest.wrapping_add(ten_k);
    }
}

/// Generates V = buffer * 10^decimal_exponent with M- <= V <= M+; M- and M+
/// share an exponent in [-60, -32].
fn grisu2_digit_gen(
    buffer: &mut Vec<u8>,
    decimal_exponent: &mut i32,
    m_minus: DiyFp,
    w: DiyFp,
    m_plus: DiyFp,
) {
    let mut delta = DiyFp::sub(m_plus, m_minus).f;
    let mut dist = DiyFp::sub(m_plus, w).f;

    // M+ = p1 + p2 * 2^e, p1 the integral part and p2 the fraction.
    let shift = (-m_plus.e) as u32;
    let one = DiyFp::new(1_u64 << shift, m_plus.e);
    let mut p1 = (m_plus.f >> shift) as u32;
    let mut p2 = m_plus.f & (one.f - 1);

    // 1) The digits of the integral part, stopping as soon as the rest is
    // within delta.
    let (k, mut pow10) = find_largest_pow10(p1);
    let mut n = k;
    while n > 0 {
        let d = p1 / pow10;
        let r = p1 % pow10;
        buffer.push(b'0' + d as u8);
        p1 = r;
        n -= 1;
        let rest = (u64::from(p1) << shift).wrapping_add(p2);
        if rest <= delta {
            *decimal_exponent += n;
            // pow10 = 10^n is now one unit in the last place of V.
            let ten_n = u64::from(pow10) << shift;
            grisu2_round(buffer, dist, delta, rest, ten_n);
            return;
        }
        pow10 /= 10;
    }

    // 2) The digits of the fractional part, until the remainder is within
    // delta (scaled by 10 with each digit).
    let mut m = 0;
    loop {
        p2 = p2.wrapping_mul(10);
        let d = p2 >> shift;
        let r = p2 & (one.f - 1);
        buffer.push(b'0' + d as u8);
        p2 = r;
        m += 1;
        delta = delta.wrapping_mul(10);
        dist = dist.wrapping_mul(10);
        if p2 <= delta {
            break;
        }
    }
    *decimal_exponent -= m;
    // One unit in the last place is now 10^-m, which is one.f with delta and
    // dist scaled by 10^m.
    let ten_m = one.f;
    grisu2_round(buffer, dist, delta, p2, ten_m);
}

/// The digits and decimal exponent of `value`, finite and positive.
fn grisu2(value: f64) -> (Vec<u8>, i32) {
    let w = compute_boundaries(value);
    let cached = cached_power_for_binary_exponent(w.plus.e);
    let c_minus_k = DiyFp::new(cached.f, cached.e);

    let w_scaled = DiyFp::mul(w.w, c_minus_k);
    let w_minus = DiyFp::mul(w.minus, c_minus_k);
    let w_plus = DiyFp::mul(w.plus, c_minus_k);

    // The products are off by less than an ulp each way; any number in
    // [M-, M+] rounds to w when read back.
    let m_minus = DiyFp::new(w_minus.f.wrapping_add(1), w_minus.e);
    let m_plus = DiyFp::new(w_plus.f.wrapping_sub(1), w_plus.e);

    let mut decimal_exponent = -cached.k;
    let mut buffer = Vec::with_capacity(17);
    grisu2_digit_gen(
        &mut buffer,
        &mut decimal_exponent,
        m_minus,
        w_scaled,
        m_plus,
    );
    (buffer, decimal_exponent)
}

/// At least two exponent digits, as printf("%g") writes them.
fn append_exponent(out: &mut String, exponent: i32) {
    out.push(if exponent < 0 { '-' } else { '+' });
    let k = exponent.unsigned_abs();
    if k < 10 {
        out.push('0');
        out.push(char::from(b'0' + k as u8));
    } else if k < 100 {
        out.push(char::from(b'0' + (k / 10) as u8));
        out.push(char::from(b'0' + (k % 10) as u8));
    } else {
        out.push(char::from(b'0' + (k / 100) as u8));
        out.push(char::from(b'0' + (k % 100 / 10) as u8));
        out.push(char::from(b'0' + (k % 10) as u8));
    }
}

/// v = digits * 10^decimal_exponent, in fixed notation in [10^min_exp,
/// 10^max_exp) and in exponential notation otherwise.
fn format_buffer(
    out: &mut String,
    digits: &[u8],
    decimal_exponent: i32,
    min_exp: i32,
    max_exp: i32,
) {
    let k = digits.len() as i32;
    let n = k + decimal_exponent;
    let text = |bytes: &[u8]| String::from_utf8_lossy(bytes).into_owned();

    if k <= n && n <= max_exp {
        // digits[000].0 - made to look like a floating-point number.
        out.push_str(&text(digits));
        out.push_str(&"0".repeat((n - k) as usize));
        out.push_str(".0");
        return;
    }
    if 0 < n && n <= max_exp {
        // dig.its
        out.push_str(&text(&digits[..n as usize]));
        out.push('.');
        out.push_str(&text(&digits[n as usize..]));
        return;
    }
    if min_exp < n && n <= 0 {
        // 0.[000]digits
        out.push_str("0.");
        out.push_str(&"0".repeat((-n) as usize));
        out.push_str(&text(digits));
        return;
    }
    if k == 1 {
        // dE+123
        out.push_str(&text(digits));
    } else {
        // d.igitsE+123
        out.push_str(&text(&digits[..1]));
        out.push('.');
        out.push_str(&text(&digits[1..]));
    }
    out.push('e');
    append_exponent(out, n - 1);
}

/// nlohmann::detail::to_chars for a finite double.
fn to_chars(mut value: f64) -> String {
    let mut out = String::new();
    // signbit rather than < 0, so that -0 keeps its sign.
    if value.is_sign_negative() {
        value = -value;
        out.push('-');
    }
    if value == 0.0 {
        // Made to look like a floating-point number (nlohmann #362, #378).
        out.push_str("0.0");
        return out;
    }
    let (digits, decimal_exponent) = grisu2(value);
    // Like printf("%.*g"), with digits10 (15) as the largest fixed exponent
    // for compatibility with nlohmann's version 2.
    const MIN_EXP: i32 = -4;
    const MAX_EXP: i32 = 15;
    format_buffer(&mut out, &digits, decimal_exponent, MIN_EXP, MAX_EXP);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // Worked out by hand from to_chars.hpp's format_buffer and append_exponent
    // (fixed for a decimal exponent in (-4, 15], at least one fractional digit,
    // two exponent digits) for values whose shortest digits are plain - not
    // taken from a run of this port.
    #[test]
    fn doubles_are_written_as_nlohmann_writes_them() {
        assert_eq!(dump_float(0.0), "0.0");
        assert_eq!(dump_float(-0.0), "-0.0");
        assert_eq!(dump_float(7.0), "7.0");
        assert_eq!(dump_float(1.5), "1.5");
        assert_eq!(dump_float(-2.25), "-2.25");
        assert_eq!(dump_float(0.1), "0.1");
        assert_eq!(dump_float(0.0001), "0.0001");
        assert_eq!(dump_float(1e-5), "1e-05");
        assert_eq!(dump_float(1e15), "1e+15");
        assert_eq!(dump_float(1e14), "100000000000000.0");
        assert_eq!(dump_float(1e20), "1e+20");
        assert_eq!(dump_float(123.456), "123.456");
        assert_eq!(dump_float(f64::NAN), "null");
        assert_eq!(dump_float(f64::INFINITY), "null");
        assert_eq!(dump_float(1.5e20), "1.5e+20");
    }

    #[test]
    fn objects_are_written_with_their_keys_sorted() {
        let mut map = Map::new();
        map.insert(String::from("b"), Value::from(1_u64));
        map.insert(String::from("a"), real(2.0));
        map.insert(String::from("c"), Value::from("x\"\n\u{1}"));
        assert_eq!(
            dump(&Value::Object(map)),
            "{\"a\":2.0,\"b\":1,\"c\":\"x\\\"\\n\\u0001\"}"
        );
    }
}
