//! Centralised unit conversions.
//!
//! Every length and angle conversion in Katana goes through this module so
//! that the definition of a unit exists exactly once. The length DEFINITIONS
//! themselves are one layer down, in `katana_math::unit_ratio`, so that the
//! survey parsers - which may not see geodesy - use the same numbers rather
//! than a copy of them; this module is the catalogue built on them.
//!
//! Numerical policy
//! * Length units are stored as EXACT rational numbers of metres
//!   (international foot = 381/1250 m, US survey foot = 1200/3937 m). A
//!   conversion is evaluated as (value * p) / q with p and q exactly
//!   representable integers, i.e. at most two correctly rounded operations
//!   (one when value * p is exact, which is the case for ordinary
//!   coordinates).
//! * Angle units other than the radian are exact fractions of a full turn
//!   (360 deg = 400 gon = 1 296 000 arc-seconds); conversions between them
//!   never touch pi. Conversions involving the radian use the factors
//!   (2*pi)/n and n/(2*pi), which for the degree are bit-identical to
//!   `katana_math::DEG_TO_RAD` and `RAD_TO_DEG`.
//! * There is NO default unit: an unknown unit name is an error, never a
//!   silent factor of 1.
//!
//! "foot" / "ft" ALWAYS mean the international foot (0.3048 m exactly),
//! matching EPSG ("foot", EPSG:9002) and PROJ ("ft"). The US survey foot must
//! be named explicitly ("ftUS", "us-ft", "US survey foot"). The two differ by
//! 2 ppm, which is 4 ft at a state-plane coordinate of 2 000 000 ft.

use katana_core::{Error, ErrorCode, Result};
use katana_math::numerics::{TWO_PI, nearly_equal, tolerance};
use katana_math::unit_ratio::{UnitRatio, ratio_value, scale_by_ratio, units as definitions};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum LengthUnit {
    Millimetre,
    Centimetre,
    Metre,
    Kilometre,
    /// 0.0254 m.
    InternationalInch,
    /// 0.3048 m (EPSG:9002).
    InternationalFoot,
    /// 0.9144 m.
    InternationalYard,
    /// 66 international feet = 20.1168 m (EPSG:9097).
    InternationalChain,
    /// 1/100 chain = 0.201168 m (EPSG:9098).
    InternationalLink,
    /// 5280 international feet = 1609.344 m.
    InternationalMile,
    /// 1852 m.
    NauticalMile,
    /// 1200/3937 m (EPSG:9003).
    UsSurveyFoot,
    /// 66 US survey feet (EPSG:9033).
    UsSurveyChain,
    /// 1/100 US survey chain (EPSG:9034).
    UsSurveyLink,
    /// 5280 US survey feet (EPSG:9035).
    UsSurveyMile,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum AngleUnit {
    Radian,
    Degree,
    /// Also called grad / gradian: 400 per turn.
    Gon,
    ArcMinute,
    ArcSecond,
}

mod sealed {
    pub trait Sealed: Copy {
        fn name_text(self) -> &'static str;
        fn symbol_text(self) -> &'static str;
    }
}

/// A unit of either catalogue, so that `to_string` and `abbreviation` take
/// both, as the C++ overloads do.
pub trait Unit: sealed::Sealed {}

impl Unit for LengthUnit {}
impl Unit for AngleUnit {}

// The defining constants are katana_math::unit_ratio::units, where every layer
// can see them; this maps the catalogue onto them. The match is exhaustive, so
// an enumerator added without a definition does not compile.
const fn length_ratio(unit: LengthUnit) -> UnitRatio {
    match unit {
        LengthUnit::Millimetre => definitions::MILLIMETRE,
        LengthUnit::Centimetre => definitions::CENTIMETRE,
        LengthUnit::Metre => definitions::METRE,
        LengthUnit::Kilometre => definitions::KILOMETRE,
        LengthUnit::InternationalInch => definitions::INTERNATIONAL_INCH,
        LengthUnit::InternationalFoot => definitions::INTERNATIONAL_FOOT,
        LengthUnit::InternationalYard => definitions::INTERNATIONAL_YARD,
        LengthUnit::InternationalChain => definitions::INTERNATIONAL_CHAIN,
        LengthUnit::InternationalLink => definitions::INTERNATIONAL_LINK,
        LengthUnit::InternationalMile => definitions::INTERNATIONAL_MILE,
        LengthUnit::NauticalMile => definitions::NAUTICAL_MILE,
        LengthUnit::UsSurveyFoot => definitions::US_SURVEY_FOOT,
        LengthUnit::UsSurveyChain => definitions::US_SURVEY_CHAIN,
        LengthUnit::UsSurveyLink => definitions::US_SURVEY_LINK,
        LengthUnit::UsSurveyMile => definitions::US_SURVEY_MILE,
    }
}

// Units per full turn; 0 marks the radian, whose size is irrational.
const fn units_per_turn(unit: AngleUnit) -> i64 {
    match unit {
        AngleUnit::Radian => 0,
        AngleUnit::Degree => 360,
        AngleUnit::Gon => 400,
        AngleUnit::ArcMinute => 21600,
        AngleUnit::ArcSecond => 1296000,
    }
}

// Lower-cases ASCII and drops spaces, tabs, '-' and '_' so that spelling
// variants of one name compare equal.
fn normalise_unit_text(text: &str) -> Vec<u8> {
    let mut key = Vec::with_capacity(text.len());
    for &byte in text.as_bytes() {
        if matches!(byte, b' ' | b'\t' | b'-' | b'_') {
            continue;
        }
        key.push(byte.to_ascii_lowercase());
    }
    key
}

// Keys are already normalised.
const LENGTH_ALIASES: &[(&str, LengthUnit)] = &[
    ("mm", LengthUnit::Millimetre),
    ("millimetre", LengthUnit::Millimetre),
    ("millimetres", LengthUnit::Millimetre),
    ("millimeter", LengthUnit::Millimetre),
    ("millimeters", LengthUnit::Millimetre),
    ("cm", LengthUnit::Centimetre),
    ("centimetre", LengthUnit::Centimetre),
    ("centimetres", LengthUnit::Centimetre),
    ("centimeter", LengthUnit::Centimetre),
    ("centimeters", LengthUnit::Centimetre),
    ("m", LengthUnit::Metre),
    ("metre", LengthUnit::Metre),
    ("metres", LengthUnit::Metre),
    ("meter", LengthUnit::Metre),
    ("meters", LengthUnit::Metre),
    ("km", LengthUnit::Kilometre),
    ("kilometre", LengthUnit::Kilometre),
    ("kilometres", LengthUnit::Kilometre),
    ("kilometer", LengthUnit::Kilometre),
    ("kilometers", LengthUnit::Kilometre),
    ("in", LengthUnit::InternationalInch),
    ("inch", LengthUnit::InternationalInch),
    ("inches", LengthUnit::InternationalInch),
    ("ft", LengthUnit::InternationalFoot),
    ("ift", LengthUnit::InternationalFoot),
    ("foot", LengthUnit::InternationalFoot),
    ("feet", LengthUnit::InternationalFoot),
    ("internationalfoot", LengthUnit::InternationalFoot),
    ("internationalfeet", LengthUnit::InternationalFoot),
    ("yd", LengthUnit::InternationalYard),
    ("yard", LengthUnit::InternationalYard),
    ("yards", LengthUnit::InternationalYard),
    ("ch", LengthUnit::InternationalChain),
    ("chain", LengthUnit::InternationalChain),
    ("chains", LengthUnit::InternationalChain),
    ("lk", LengthUnit::InternationalLink),
    ("link", LengthUnit::InternationalLink),
    ("links", LengthUnit::InternationalLink),
    ("mi", LengthUnit::InternationalMile),
    ("mile", LengthUnit::InternationalMile),
    ("miles", LengthUnit::InternationalMile),
    ("statutemile", LengthUnit::InternationalMile),
    ("nmi", LengthUnit::NauticalMile),
    ("kmi", LengthUnit::NauticalMile), // PROJ unit id
    ("nauticalmile", LengthUnit::NauticalMile),
    ("nauticalmiles", LengthUnit::NauticalMile),
    ("ftus", LengthUnit::UsSurveyFoot),
    ("usft", LengthUnit::UsSurveyFoot), // PROJ unit id "us-ft"
    ("sft", LengthUnit::UsSurveyFoot),
    ("footus", LengthUnit::UsSurveyFoot), // ESRI "Foot_US"
    ("ussurveyfoot", LengthUnit::UsSurveyFoot),
    ("ussurveyfeet", LengthUnit::UsSurveyFoot),
    ("surveyfoot", LengthUnit::UsSurveyFoot),
    ("surveyfeet", LengthUnit::UsSurveyFoot),
    ("chus", LengthUnit::UsSurveyChain),
    ("usch", LengthUnit::UsSurveyChain), // PROJ unit id "us-ch"
    ("ussurveychain", LengthUnit::UsSurveyChain),
    ("ussurveychains", LengthUnit::UsSurveyChain),
    ("lkus", LengthUnit::UsSurveyLink),
    ("ussurveylink", LengthUnit::UsSurveyLink),
    ("ussurveylinks", LengthUnit::UsSurveyLink),
    ("mius", LengthUnit::UsSurveyMile),
    ("usmi", LengthUnit::UsSurveyMile), // PROJ unit id "us-mi"
    ("ussurveymile", LengthUnit::UsSurveyMile),
    ("ussurveymiles", LengthUnit::UsSurveyMile),
];

const ANGLE_ALIASES: &[(&str, AngleUnit)] = &[
    ("rad", AngleUnit::Radian),
    ("radian", AngleUnit::Radian),
    ("radians", AngleUnit::Radian),
    ("deg", AngleUnit::Degree),
    ("degree", AngleUnit::Degree),
    ("degrees", AngleUnit::Degree),
    ("\u{b0}", AngleUnit::Degree), // the degree sign, UTF-8 C2 B0
    ("gon", AngleUnit::Gon),
    ("gons", AngleUnit::Gon),
    ("grad", AngleUnit::Gon),
    ("grads", AngleUnit::Gon),
    ("gradian", AngleUnit::Gon),
    ("gradians", AngleUnit::Gon),
    ("arcmin", AngleUnit::ArcMinute),
    ("arcminute", AngleUnit::ArcMinute),
    ("arcminutes", AngleUnit::ArcMinute),
    ("'", AngleUnit::ArcMinute),
    ("arcsec", AngleUnit::ArcSecond),
    ("arcsecond", AngleUnit::ArcSecond),
    ("arcseconds", AngleUnit::ArcSecond),
    ("\"", AngleUnit::ArcSecond),
];

fn parse_unit<U: Copy>(text: &str, aliases: &[(&str, U)], quantity: &str) -> Result<U> {
    let key = normalise_unit_text(text);
    for &(alias, unit) in aliases {
        if alias.as_bytes() == key.as_slice() {
            return Ok(unit);
        }
    }
    Err(
        Error::new(ErrorCode::ParseFailure, format!("unknown {quantity} unit"))
            .with_context(format!("input='{text}'")),
    )
}

// Every enumerator, so that a new unit only has to be added here and in
// length_ratio() (whose exhaustive match flags an omission).
const ALL_LENGTH_UNITS: [LengthUnit; 15] = [
    LengthUnit::Millimetre,
    LengthUnit::Centimetre,
    LengthUnit::Metre,
    LengthUnit::Kilometre,
    LengthUnit::InternationalInch,
    LengthUnit::InternationalFoot,
    LengthUnit::InternationalYard,
    LengthUnit::InternationalChain,
    LengthUnit::InternationalLink,
    LengthUnit::InternationalMile,
    LengthUnit::NauticalMile,
    LengthUnit::UsSurveyFoot,
    LengthUnit::UsSurveyChain,
    LengthUnit::UsSurveyLink,
    LengthUnit::UsSurveyMile,
];

/// Metres in one `unit`: the correctly rounded value of the exact rational.
pub fn metres_per_unit(unit: LengthUnit) -> f64 {
    ratio_value(length_ratio(unit))
}

/// Radians in one `unit`.
pub fn radians_per_unit(unit: AngleUnit) -> f64 {
    let per_turn = units_per_turn(unit);
    if per_turn == 0 {
        1.0
    } else {
        TWO_PI / per_turn as f64
    }
}

/// The unit whose size is `metres`, e.g. a conversion factor read from a WKT
/// LENGTHUNIT or a LandXML header. Compared with `tolerance::RELATIVE`, which
/// is 600 times finer than the 2 ppm separating the two feet, so they are
/// never confused; `None` when no known unit matches.
pub fn length_unit_from_metres_per_unit(metres: f64) -> Option<LengthUnit> {
    ALL_LENGTH_UNITS
        .into_iter()
        .find(|&unit| nearly_equal(metres, metres_per_unit(unit), tolerance::RELATIVE))
}

pub fn convert_length(value: f64, from: LengthUnit, to: LengthUnit) -> f64 {
    if from == to {
        return value; // guarantees bit-exact identity
    }
    let source = length_ratio(from);
    let target = length_ratio(to);
    scale_by_ratio(
        value,
        source.numerator * target.denominator,
        source.denominator * target.numerator,
    )
}

pub fn convert_angle(value: f64, from: AngleUnit, to: AngleUnit) -> f64 {
    if from == to {
        return value;
    }
    let from_per_turn = units_per_turn(from);
    let to_per_turn = units_per_turn(to);
    if from_per_turn == 0 {
        // (n / 2pi) is a single correctly rounded constant; for the degree it
        // is bit-identical to RAD_TO_DEG because 2*PI is exact.
        return value * (to_per_turn as f64 / TWO_PI);
    }
    if to_per_turn == 0 {
        return value * (TWO_PI / from_per_turn as f64);
    }
    scale_by_ratio(value, to_per_turn, from_per_turn)
}

/// EPSG-style names ("metre", "foot", "US survey foot", "degree", "grad", ...).
pub fn to_string<U: Unit>(unit: U) -> &'static str {
    U::name_text(unit)
}

/// Short symbols ("m", "ft", "ftUS", "deg", "gon", ...).
pub fn abbreviation<U: Unit>(unit: U) -> &'static str {
    U::symbol_text(unit)
}

impl sealed::Sealed for LengthUnit {
    fn name_text(self) -> &'static str {
        match self {
            LengthUnit::Millimetre => "millimetre",
            LengthUnit::Centimetre => "centimetre",
            LengthUnit::Metre => "metre",
            LengthUnit::Kilometre => "kilometre",
            LengthUnit::InternationalInch => "inch",
            LengthUnit::InternationalFoot => "foot",
            LengthUnit::InternationalYard => "yard",
            LengthUnit::InternationalChain => "chain",
            LengthUnit::InternationalLink => "link",
            LengthUnit::InternationalMile => "Statute mile",
            LengthUnit::NauticalMile => "nautical mile",
            LengthUnit::UsSurveyFoot => "US survey foot",
            LengthUnit::UsSurveyChain => "US survey chain",
            LengthUnit::UsSurveyLink => "US survey link",
            LengthUnit::UsSurveyMile => "US survey mile",
        }
    }

    fn symbol_text(self) -> &'static str {
        match self {
            LengthUnit::Millimetre => "mm",
            LengthUnit::Centimetre => "cm",
            LengthUnit::Metre => "m",
            LengthUnit::Kilometre => "km",
            LengthUnit::InternationalInch => "in",
            LengthUnit::InternationalFoot => "ft",
            LengthUnit::InternationalYard => "yd",
            LengthUnit::InternationalChain => "ch",
            LengthUnit::InternationalLink => "lk",
            LengthUnit::InternationalMile => "mi",
            LengthUnit::NauticalMile => "nmi",
            LengthUnit::UsSurveyFoot => "ftUS",
            LengthUnit::UsSurveyChain => "chUS",
            LengthUnit::UsSurveyLink => "lkUS",
            LengthUnit::UsSurveyMile => "miUS",
        }
    }
}

impl sealed::Sealed for AngleUnit {
    fn name_text(self) -> &'static str {
        match self {
            AngleUnit::Radian => "radian",
            AngleUnit::Degree => "degree",
            AngleUnit::Gon => "grad",
            AngleUnit::ArcMinute => "arc-minute",
            AngleUnit::ArcSecond => "arc-second",
        }
    }

    fn symbol_text(self) -> &'static str {
        match self {
            AngleUnit::Radian => "rad",
            AngleUnit::Degree => "deg",
            AngleUnit::Gon => "gon",
            AngleUnit::ArcMinute => "arcmin",
            AngleUnit::ArcSecond => "arcsec",
        }
    }
}

/// Parses a unit name or symbol. Matching ignores case, spaces, '-' and '_',
/// so "US survey foot", "us-ft", "ftUS" and "Foot_US" all name the US survey
/// foot. Accepts EPSG names, PROJ unit ids and common survey abbreviations.
/// Unknown or empty text fails with `ErrorCode::ParseFailure`.
pub fn parse_length_unit(text: &str) -> Result<LengthUnit> {
    parse_unit(text, LENGTH_ALIASES, "length")
}

pub fn parse_angle_unit(text: &str) -> Result<AngleUnit> {
    parse_unit(text, ANGLE_ALIASES, "angle")
}
