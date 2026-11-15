#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnitRatio {
    pub numerator: i64,
    pub denominator: i64,
}

impl Default for UnitRatio {
    fn default() -> Self {
        Self::new(1, 1)
    }
}

impl UnitRatio {
    pub const fn new(numerator: i64, denominator: i64) -> Self {
        Self {
            numerator,
            denominator,
        }
    }
}

pub mod units {
    use super::UnitRatio;

    pub const MILLIMETRE: UnitRatio = UnitRatio::new(1, 1000);
    pub const CENTIMETRE: UnitRatio = UnitRatio::new(1, 100);
    pub const METRE: UnitRatio = UnitRatio::new(1, 1);
    pub const KILOMETRE: UnitRatio = UnitRatio::new(1000, 1);
    pub const INTERNATIONAL_INCH: UnitRatio = UnitRatio::new(127, 5000);
    pub const INTERNATIONAL_FOOT: UnitRatio = UnitRatio::new(381, 1250);
    pub const INTERNATIONAL_YARD: UnitRatio = UnitRatio::new(1143, 1250);
    pub const INTERNATIONAL_CHAIN: UnitRatio = UnitRatio::new(12573, 625);
    pub const INTERNATIONAL_LINK: UnitRatio = UnitRatio::new(12573, 62500);
    pub const INTERNATIONAL_MILE: UnitRatio = UnitRatio::new(201168, 125);
    pub const NAUTICAL_MILE: UnitRatio = UnitRatio::new(1852, 1);
    pub const US_SURVEY_FOOT: UnitRatio = UnitRatio::new(1200, 3937);
    pub const US_SURVEY_CHAIN: UnitRatio = UnitRatio::new(79200, 3937);
    pub const US_SURVEY_LINK: UnitRatio = UnitRatio::new(792, 3937);
    pub const US_SURVEY_MILE: UnitRatio = UnitRatio::new(6336000, 3937);
}

pub const fn scale_by_ratio(value: f64, numerator: i64, denominator: i64) -> f64 {
    let divisor = greatest_common_divisor(numerator, denominator);
    let reduced_numerator = (numerator / divisor) as f64;
    let reduced_denominator = (denominator / divisor) as f64;
    (value * reduced_numerator) / reduced_denominator
}

pub const fn to_metres(value: f64, unit: UnitRatio) -> f64 {
    scale_by_ratio(value, unit.numerator, unit.denominator)
}

pub const fn from_metres(metres: f64, unit: UnitRatio) -> f64 {
    scale_by_ratio(metres, unit.denominator, unit.numerator)
}

pub const fn ratio_value(unit: UnitRatio) -> f64 {
    unit.numerator as f64 / unit.denominator as f64
}

const fn greatest_common_divisor(first: i64, second: i64) -> i64 {
    let mut first = first
        .checked_abs()
        .expect("ratio numerator magnitude must fit in i64");
    let mut second = second
        .checked_abs()
        .expect("ratio denominator magnitude must fit in i64");
    while second != 0 {
        let remainder = first % second;
        first = second;
        second = remainder;
    }
    first
}
