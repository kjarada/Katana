//! Subsurface utility quality levels, after AS 5488.1-2019 "Classification of
//! subsurface utility information (SUI)". docs/subsurface_utilities.md is the
//! record of how the standard was read and where Katana is deliberately
//! stricter than it.
//!
//! The standard grades every piece of information about a buried service by
//! HOW it was obtained, not by how good it looks:
//!
//! - QL-D  from existing records or anecdote; no position is measured.
//! - QL-C  a surveyed surface feature (a pit, a valve, a marker post) correlated
//!   with the records; the service itself is still inferred.
//! - QL-B  the service is detected from the surface - electromagnetic location,
//!   ground penetrating radar and the like - and its position surveyed,
//!   within a stated horizontal and (where a depth is given) vertical
//!   tolerance.
//! - QL-A  the service is exposed and seen - potholing, non-destructive
//!   excavation, an open trench - and surveyed in three dimensions
//!   within a tight absolute tolerance.
//!
//! The ordering is the point of the enumeration: a worse level compares LESS
//! than a better one, so "the weakest of two" is `min` and a claim is
//! supported when claimed <= attainable.
//!
//! Tolerances are carried as data rather than constants. The defaults are the
//! published figures (QL-A +/-50 mm horizontal and vertical, QL-B +/-300 mm
//! horizontal and +/-500 mm vertical), but a client specification may be
//! tighter, and a future edition may differ; a program that hard-codes them
//! would silently certify against the wrong numbers.

use super::table::key;

/// The spelling of an enumerator of the subsurface tools: what the C++'s
/// `toString` overloads return, one implementation per enumeration.
pub trait Named: Copy {
    fn name(self) -> &'static str;
}

/// The C++'s `toString`, for every enumeration of the subsurface tools.
pub fn to_string<T: Named>(value: T) -> &'static str {
    value.name()
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum QualityLevel {
    /// Records / anecdotal.
    #[default]
    D = 0,
    /// Surface feature correlation.
    C = 1,
    /// Detected and surveyed.
    B = 2,
    /// Exposed and surveyed in 3D.
    A = 3,
}

/// "QL-A" .. "QL-D": the spelling the standard and every deliverable uses.
impl Named for QualityLevel {
    fn name(self) -> &'static str {
        match self {
            QualityLevel::A => "QL-A",
            QualityLevel::B => "QL-B",
            QualityLevel::C => "QL-C",
            QualityLevel::D => "QL-D",
        }
    }
}

/// Accepts "QL-A", "QLA", "QL A", "A" and "Quality Level A" in any letter
/// case, with blanks around. None for anything else, including "E",
/// "Unknown" and an empty string: an unknown quality level is not QL-D, it is
/// a record nobody graded.
pub fn parse_quality_level(text: &str) -> Option<QualityLevel> {
    // The part of `text` that is the level letter, with "QL", '-', '_' and
    // blanks stripped.
    let letters = key(text);
    let letters = if letters.len() == 3 && letters.starts_with("ql") {
        &letters[2..]
    } else if letters.len() == 13 && letters.starts_with("qualitylevel") {
        &letters[12..]
    } else {
        letters.as_str()
    };
    if letters.len() != 1 {
        return None;
    }
    match letters.as_bytes()[0] {
        b'a' => Some(QualityLevel::A),
        b'b' => Some(QualityLevel::B),
        b'c' => Some(QualityLevel::C),
        b'd' => Some(QualityLevel::D),
        _ => None,
    }
}

/// How a position was obtained. The quality level a record may claim is capped
/// by its method before any tolerance is looked at: no accuracy makes a
/// radar pick into a QL-A observation, because QL-A means the service was SEEN.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LocationMethod {
    /// The method was not recorded: nothing supports better than QL-D.
    Unknown,
    /// Plans, GIS, as-constructed drawings.
    Records,
    /// Verbal, site knowledge.
    Anecdotal,
    /// A pit, valve, marker or pole surveyed and correlated.
    SurfaceFeature,
    /// Active or passive EML, sonde in a duct.
    ElectromagneticLocation,
    /// GPR.
    GroundPenetratingRadar,
    /// Acoustic, magnetometry, seismic ...
    OtherGeophysical,
    /// Vacuum or hydro excavation: a pothole.
    NonDestructiveExcavation,
    /// An open trench, the service in view.
    OpenExcavation,
}

impl Named for LocationMethod {
    fn name(self) -> &'static str {
        match self {
            LocationMethod::Unknown => "unknown method",
            LocationMethod::Records => "records",
            LocationMethod::Anecdotal => "anecdotal",
            LocationMethod::SurfaceFeature => "surface feature",
            LocationMethod::ElectromagneticLocation => "electromagnetic location",
            LocationMethod::GroundPenetratingRadar => "ground penetrating radar",
            LocationMethod::OtherGeophysical => "other geophysical",
            LocationMethod::NonDestructiveExcavation => "non-destructive excavation",
            LocationMethod::OpenExcavation => "open excavation",
        }
    }
}

/// Accepts the enumerator names, the field abbreviations surveyors use -
/// "records", "anecdotal", "surface", "EML", "GPR", "geophysical", "NDD",
/// "pothole", "vac", "trench" - and the Locate Method names of AS 5488.2 as
/// delivery schemas spell them: "Archive Drawings and Plans" and "Geographic
/// Information System" are Records, "Electronic Detection" is EML,
/// "Potholing" is non-destructive excavation, "Survey" (a surveyed feature)
/// is SurfaceFeature. Any letter case; None otherwise.
pub fn parse_location_method(text: &str) -> Option<LocationMethod> {
    const NAMES: [(&str, LocationMethod); 29] = [
        ("unknown", LocationMethod::Unknown),
        ("records", LocationMethod::Records),
        ("archivedrawingsandplans", LocationMethod::Records),
        ("geographicinformationsystem", LocationMethod::Records),
        ("gis", LocationMethod::Records),
        (
            "electronicdetection",
            LocationMethod::ElectromagneticLocation,
        ),
        ("potholing", LocationMethod::NonDestructiveExcavation),
        ("survey", LocationMethod::SurfaceFeature),
        ("record", LocationMethod::Records),
        ("plans", LocationMethod::Records),
        ("anecdotal", LocationMethod::Anecdotal),
        ("verbal", LocationMethod::Anecdotal),
        ("surface", LocationMethod::SurfaceFeature),
        ("surfacefeature", LocationMethod::SurfaceFeature),
        ("feature", LocationMethod::SurfaceFeature),
        ("eml", LocationMethod::ElectromagneticLocation),
        ("electromagnetic", LocationMethod::ElectromagneticLocation),
        (
            "electromagneticlocation",
            LocationMethod::ElectromagneticLocation,
        ),
        ("gpr", LocationMethod::GroundPenetratingRadar),
        (
            "groundpenetratingradar",
            LocationMethod::GroundPenetratingRadar,
        ),
        ("geophysical", LocationMethod::OtherGeophysical),
        ("othergeophysical", LocationMethod::OtherGeophysical),
        ("ndd", LocationMethod::NonDestructiveExcavation),
        ("nde", LocationMethod::NonDestructiveExcavation),
        ("pothole", LocationMethod::NonDestructiveExcavation),
        ("vac", LocationMethod::NonDestructiveExcavation),
        (
            "nondestructiveexcavation",
            LocationMethod::NonDestructiveExcavation,
        ),
        ("trench", LocationMethod::OpenExcavation),
        ("openexcavation", LocationMethod::OpenExcavation),
    ];
    // Blanks, '-' and '_' are not significant: "non-destructive excavation",
    // "Non_Destructive_Excavation" and "NonDestructiveExcavation" are one name.
    let wanted = key(text);
    NAMES
        .iter()
        .find(|(name, _)| *name == wanted)
        .map(|(_, method)| *method)
}

/// The best level the method can ever support.
pub fn maximum_quality_level(method: LocationMethod) -> QualityLevel {
    match method {
        LocationMethod::Unknown | LocationMethod::Records | LocationMethod::Anecdotal => {
            QualityLevel::D
        }
        LocationMethod::SurfaceFeature => QualityLevel::C,
        LocationMethod::ElectromagneticLocation
        | LocationMethod::GroundPenetratingRadar
        | LocationMethod::OtherGeophysical => QualityLevel::B,
        LocationMethod::NonDestructiveExcavation | LocationMethod::OpenExcavation => {
            QualityLevel::A
        }
    }
}

/// Positional tolerance of one quality level, in metres, as a +/- figure.
/// None means the level states none: QL-C and QL-D positions are not
/// measurements of the service, so there is nothing to test them against.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Tolerance {
    pub horizontal: Option<f64>,
    pub vertical: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct QualityLevelTolerances {
    pub a: Tolerance,
    pub b: Tolerance,
    pub c: Tolerance,
    pub d: Tolerance,
}

impl Default for QualityLevelTolerances {
    /// AS 5488.1's published figures: QL-A +/-50 mm horizontal and vertical,
    /// QL-B +/-300 mm horizontal and +/-500 mm vertical.
    fn default() -> Self {
        Self {
            a: Tolerance {
                horizontal: Some(0.050),
                vertical: Some(0.050),
            },
            b: Tolerance {
                horizontal: Some(0.300),
                vertical: Some(0.500),
            },
            c: Tolerance::default(),
            d: Tolerance::default(),
        }
    }
}

impl QualityLevelTolerances {
    pub fn of(&self, level: QualityLevel) -> &Tolerance {
        match level {
            QualityLevel::A => &self.a,
            QualityLevel::B => &self.b,
            QualityLevel::C => &self.c,
            QualityLevel::D => &self.d,
        }
    }
}

/// What a position's own uncertainty supports. Uncertainties are +/- figures
/// in metres at the confidence the tolerances are stated at; the caller is
/// responsible for bringing an instrument's 1-sigma figure to that confidence.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PositionEvidence {
    pub method: LocationMethod,
    /// None: not assessed.
    pub horizontal_uncertainty: Option<f64>,
    /// None: no level, or not assessed.
    pub vertical_uncertainty: Option<f64>,
    /// A level of the service was recorded.
    pub has_level: bool,
}

impl Default for PositionEvidence {
    fn default() -> Self {
        Self {
            method: LocationMethod::Records,
            horizontal_uncertainty: None,
            vertical_uncertainty: None,
            has_level: false,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Classification {
    pub level: QualityLevel,
    /// False when a level was recorded but is not good to the tolerance of
    /// `level` - a QL-B detection whose depth estimate is outside +/-500 mm
    /// keeps its QL-B plan position, and its depth must not be relied on.
    pub level_qualified: bool,
    /// Why the result is below the method's ceiling, or why its level is not
    /// qualified: one sentence each, in the order the rules were applied.
    /// Empty when the ceiling was reached in full.
    pub reasons: Vec<String>,
}

/// A tolerance test that treats an unassessed uncertainty as a failure. A
/// non-finite or negative uncertainty is a broken record, not a small one.
fn within(uncertainty: Option<f64>, tolerance: Option<f64>) -> bool {
    match (uncertainty, tolerance) {
        (Some(uncertainty), Some(tolerance)) => {
            uncertainty.is_finite() && uncertainty >= 0.0 && uncertainty <= tolerance
        }
        _ => false,
    }
}

/// `std::llround` as glibc's x86-64 build of it answers: half away from zero,
/// and the integer indefinite, `i64::MIN`, for a NaN or a value out of range
/// (the C++ leaves that unspecified; it reaches here for a NaN uncertainty).
fn rounded_to_integer(value: f64) -> i64 {
    // -2^63, exactly: the range of i64 is [-2^63, 2^63).
    const LOWEST: f64 = i64::MIN as f64;
    let rounded = value.round();
    if rounded.is_nan() || !(LOWEST..-LOWEST).contains(&rounded) {
        return i64::MIN;
    }
    rounded as i64
}

fn millimetres(metres: f64) -> String {
    format!("{} mm", rounded_to_integer(metres * 1000.0))
}

fn describe(what: &str, value: Option<f64>, limit: Option<f64>, level: QualityLevel) -> String {
    let mut text = format!("{what} uncertainty ");
    match value {
        Some(value) => text += &millimetres(value),
        None => text += "not assessed",
    }
    if let Some(limit) = limit {
        text += &format!(
            " against the {} tolerance of {}",
            to_string(level),
            millimetres(limit)
        );
    }
    text
}

/// The highest level `evidence` supports under `tolerances`:
/// - never above maximum_quality_level(method);
/// - QL-A needs a recorded level and BOTH uncertainties assessed and within
///   the QL-A tolerance - the standard's QL-A is a three-dimensional
///   position, so a plan position from a pothole with no level is QL-B;
/// - QL-B needs the horizontal uncertainty assessed and within tolerance;
///   an unassessed uncertainty is not assumed to be good;
/// - a detection or exposure that fails QL-B falls to QL-C: something was
///   found on site at that place, which is at least what a correlated
///   surface feature says, and nothing about its position is certified.
pub fn classify(
    evidence: &PositionEvidence,
    tolerances: &QualityLevelTolerances,
) -> Classification {
    let mut result = Classification::default();
    let ceiling = maximum_quality_level(evidence.method);
    if ceiling <= QualityLevel::C {
        // Records and surface features carry no measured position of the
        // service, so there is no tolerance to fail: the method is the answer.
        result.level = ceiling;
        return result;
    }

    if ceiling == QualityLevel::A {
        let a = &tolerances.a;
        let horizontal = within(evidence.horizontal_uncertainty, a.horizontal);
        let vertical = evidence.has_level && within(evidence.vertical_uncertainty, a.vertical);
        if horizontal && vertical {
            result.level = QualityLevel::A;
            result.level_qualified = true;
            return result;
        }
        if !evidence.has_level {
            result.reasons.push(String::from(
                "QL-A needs a level of the exposed service; none recorded",
            ));
        } else if !vertical {
            result.reasons.push(describe(
                "vertical",
                evidence.vertical_uncertainty,
                a.vertical,
                QualityLevel::A,
            ));
        }
        if !horizontal {
            result.reasons.push(describe(
                "horizontal",
                evidence.horizontal_uncertainty,
                a.horizontal,
                QualityLevel::A,
            ));
        }
    }

    let b = &tolerances.b;
    if within(evidence.horizontal_uncertainty, b.horizontal) {
        result.level = QualityLevel::B;
        result.level_qualified =
            evidence.has_level && within(evidence.vertical_uncertainty, b.vertical);
        if evidence.has_level && !result.level_qualified {
            result.reasons.push(
                describe(
                    "vertical",
                    evidence.vertical_uncertainty,
                    b.vertical,
                    QualityLevel::B,
                ) + ": the plan position stands, the level must not be relied on",
            );
        }
        return result;
    }
    result.reasons.push(describe(
        "horizontal",
        evidence.horizontal_uncertainty,
        b.horizontal,
        QualityLevel::B,
    ));
    result.level = QualityLevel::C;
    result
}
