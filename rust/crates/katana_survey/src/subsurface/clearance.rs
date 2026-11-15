//! Clearance of proposed works from located services, with each service
//! widened by the positional tolerance of its quality level.
//!
//! This is the question a subsurface utility investigation exists to answer -
//! "can we build this here?" - and the reason the quality level matters. A
//! service shown 400 mm from a proposed pipe is clear if it is QL-A (+/-50 mm),
//! may or may not be clear if it is QL-B (+/-300 mm), and says nothing at all
//! if it is QL-C or QL-D, whose drawn position is not a measurement. So the
//! answer has four values, not two, and the third and fourth tell the designer
//! which services to pothole next.

use katana_core::error::make_error;
use katana_core::{ErrorCode, Result};

use super::quality_level::{Named, QualityLevel, to_string};
use super::utility_network::{GradingSettings, UtilityLine, grade_line, top_level};
use crate::coordinate::Coordinate2;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DesignVertex {
    pub position: Coordinate2,
    /// Of the centre line of the proposed works.
    pub level: Option<f64>,
}

/// Proposed works as a centre line: a pipe, a pile row, a trench.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DesignAlignment {
    pub id: String,
    pub vertices: Vec<DesignVertex>,
    /// Half the width of the works: a pipe's outside radius, half a trench's
    /// width. Used horizontally and, for the level, vertically.
    pub half_width: f64,
}

/// The clearance the asset owners require. Not from AS 5488, which classifies
/// information and sets no separations: these come from each owner's rules.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClearanceRequirement {
    /// Metres, face to face.
    pub horizontal: f64,
    /// Metres, face to face.
    pub vertical: f64,
    /// A QL-C or QL-D service within this plan distance of the works (beyond the
    /// horizontal clearance) is Unconfirmed: its drawn position is not a
    /// measurement, so nothing short of this margin can be called clear.
    pub unverified_margin: f64,
}

impl Default for ClearanceRequirement {
    fn default() -> Self {
        Self {
            horizontal: 0.3,
            vertical: 0.3,
            unverified_margin: 2.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ClearanceStatus {
    /// Closer than required even at the drawn position.
    Conflict,
    /// QL-C / QL-D near the works: locate it before relying on it.
    Unconfirmed,
    /// Clear at the drawn position, not across the tolerance.
    WithinTolerance,
    /// Clear everywhere the service could be.
    #[default]
    Clear,
}

impl Named for ClearanceStatus {
    fn name(self) -> &'static str {
        match self {
            ClearanceStatus::Conflict => "conflict",
            ClearanceStatus::Unconfirmed => "unconfirmed",
            ClearanceStatus::WithinTolerance => "within tolerance",
            ClearanceStatus::Clear => "clear",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClearanceResult {
    pub utility_id: String,
    /// Index of the segment's first vertex.
    pub segment: usize,
    /// The segment's vertices.
    pub from_id: String,
    pub to_id: String,
    pub level: QualityLevel,
    pub status: ClearanceStatus,
    /// On the utility, where the gap is least.
    pub nearest: Coordinate2,
    /// Centre line to centre line.
    pub plan_distance: f64,
    /// Face to face at the drawn position.
    pub horizontal_gap: f64,
    pub horizontal_tolerance: Option<f64>,
    /// Face to face; None without usable levels.
    pub vertical_gap: Option<f64>,
    pub vertical_tolerance: Option<f64>,
    pub note: String,
}

struct Closest {
    distance: f64,
    /// Parameter along the first segment, [0, 1].
    s: f64,
    /// Parameter along the second segment, [0, 1].
    t: f64,
}

fn cross(an: f64, ae: f64, bn: f64, be: f64) -> f64 {
    an * be - ae * bn
}

/// Distance from p to segment a-b, and the parameter of the foot along a-b.
fn to_segment(p: &Coordinate2, a: &Coordinate2, b: &Coordinate2) -> (f64, f64) {
    let dn = b.northing - a.northing;
    let de = b.easting - a.easting;
    let length_squared = dn * dn + de * de;
    let mut t = 0.0_f64;
    if length_squared > 0.0 {
        t = ((p.northing - a.northing) * dn + (p.easting - a.easting) * de) / length_squared;
        // std::clamp's answer for every input, a NaN included.
        t = t.clamp(0.0, 1.0);
    }
    (
        (p.northing - (a.northing + t * dn)).hypot(p.easting - (a.easting + t * de)),
        t,
    )
}

/// Closest approach of two plan segments a0-a1 and b0-b1.
fn closest(a0: &Coordinate2, a1: &Coordinate2, b0: &Coordinate2, b1: &Coordinate2) -> Closest {
    let rn = a1.northing - a0.northing;
    let re = a1.easting - a0.easting;
    let sn = b1.northing - b0.northing;
    let se = b1.easting - b0.easting;
    let denominator = cross(rn, re, sn, se);
    if denominator != 0.0 {
        let qn = b0.northing - a0.northing;
        let qe = b0.easting - a0.easting;
        let s = cross(qn, qe, sn, se) / denominator;
        let t = cross(qn, qe, rn, re) / denominator;
        if (0.0..=1.0).contains(&s) && (0.0..=1.0).contains(&t) {
            return Closest {
                distance: 0.0,
                s,
                t,
            };
        }
    }
    // Apart (or parallel): the closest approach involves an end of one of them.
    let mut best = Closest {
        distance: f64::INFINITY,
        s: 0.0,
        t: 0.0,
    };
    let mut consider = |distance: f64, s: f64, t: f64| {
        if distance < best.distance {
            best = Closest { distance, s, t };
        }
    };
    let (d, t) = to_segment(a0, b0, b1);
    consider(d, 0.0, t);
    let (d, t) = to_segment(a1, b0, b1);
    consider(d, 1.0, t);
    let (d, t) = to_segment(b0, a0, a1);
    consider(d, t, 0.0);
    let (d, t) = to_segment(b1, a0, a1);
    consider(d, t, 1.0);
    best
}

fn along(a: &Coordinate2, b: &Coordinate2, t: f64) -> Coordinate2 {
    Coordinate2 {
        northing: a.northing + t * (b.northing - a.northing),
        easting: a.easting + t * (b.easting - a.easting),
    }
}

fn against(gap: f64, tolerance: f64, required: f64) -> ClearanceStatus {
    if gap - tolerance >= required {
        return ClearanceStatus::Clear;
    }
    if gap >= required {
        ClearanceStatus::WithinTolerance
    } else {
        ClearanceStatus::Conflict
    }
}

/// True when `candidate` should replace `current` as the governing result:
/// worse status first, then the smaller gap.
fn governs(candidate: &ClearanceResult, current: &ClearanceResult) -> bool {
    if candidate.status != current.status {
        return candidate.status < current.status;
    }
    candidate.horizontal_gap < current.horizontal_gap
}

fn non_negative(value: f64) -> bool {
    value.is_finite() && value >= 0.0
}

/// For every segment of every utility line, in order, the governing (worst,
/// then closest) result against the segments of `design`. Per segment rather
/// than per line because the grade changes along a line: a line whose worst
/// part is a QL-C tail far from the works may cross them on a QL-B stretch,
/// and the designer needs both.
///
/// Horizontal clearance is tested first; where it fails and both the works and
/// the service have qualified levels at the closest approach, the vertical
/// separation there can still clear it - a service crossing under a pipe at
/// depth is not a conflict.
///
/// A service with no recorded diameter is treated as a line: the gap is to its
/// centre, and the note says so.
///
/// InvalidArgument for a design with fewer than two vertices, a negative
/// half-width or requirement, or any line grade_line() refuses.
pub fn check_clearance(
    design: &DesignAlignment,
    utilities: &[UtilityLine],
    requirement: &ClearanceRequirement,
    settings: &GradingSettings,
) -> Result<Vec<ClearanceResult>> {
    if design.vertices.len() < 2 {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "the design needs at least two vertices",
            design.id.clone(),
        ));
    }
    for vertex in &design.vertices {
        if !vertex.position.northing.is_finite() || !vertex.position.easting.is_finite() {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "design vertex has a non-finite coordinate",
                design.id.clone(),
            ));
        }
    }
    if !non_negative(design.half_width)
        || !non_negative(requirement.horizontal)
        || !non_negative(requirement.vertical)
        || !non_negative(requirement.unverified_margin)
    {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "half-width and clearances must be finite and not negative",
            design.id.clone(),
        ));
    }

    let mut results = Vec::new();
    for line in utilities {
        let graded = grade_line(line, settings)?;
        let diameter = line.attributes.diameter;
        let radius = if diameter.is_finite() && diameter > 0.0 {
            diameter / 2.0
        } else {
            0.0
        };

        for segment in &graded.segments {
            let u0 = &line.vertices[segment.from];
            let u1 = &line.vertices[segment.from + 1];
            let tolerance = settings.tolerances.of(segment.level);

            // Centre-line level of the service at both ends, only where both
            // are levels the evidence qualifies.
            let mut z0 = None;
            let mut z1 = None;
            if graded.vertices[segment.from].classification.level_qualified
                && graded.vertices[segment.from + 1]
                    .classification
                    .level_qualified
                && let (Some(top0), Some(top1)) = (top_level(u0, diameter), top_level(u1, diameter))
            {
                z0 = Some(top0 - radius);
                z1 = Some(top1 - radius);
            }

            let mut governing: Option<ClearanceResult> = None;
            for j in 0..design.vertices.len() - 1 {
                let d0 = &design.vertices[j];
                let d1 = &design.vertices[j + 1];
                let approach = closest(&u0.position, &u1.position, &d0.position, &d1.position);

                let mut result = ClearanceResult {
                    utility_id: line.id.clone(),
                    segment: segment.from,
                    from_id: u0.id.clone(),
                    to_id: u1.id.clone(),
                    level: segment.level,
                    nearest: along(&u0.position, &u1.position, approach.s),
                    plan_distance: approach.distance,
                    horizontal_gap: approach.distance - radius - design.half_width,
                    horizontal_tolerance: tolerance.horizontal,
                    ..Default::default()
                };

                if let Some(horizontal) = tolerance.horizontal {
                    result.status =
                        against(result.horizontal_gap, horizontal, requirement.horizontal);
                } else if result.horizontal_gap
                    >= requirement.horizontal + requirement.unverified_margin
                {
                    result.status = ClearanceStatus::Clear;
                } else {
                    result.status = ClearanceStatus::Unconfirmed;
                    result.note = format!(
                        "{} position is not a measurement; locate the service to QL-B or QL-A \
                         before relying on this clearance",
                        to_string(segment.level)
                    );
                }

                if let (Some(z0), Some(z1), Some(level0), Some(level1), Some(vertical)) =
                    (z0, z1, d0.level, d1.level, tolerance.vertical)
                {
                    let zu = z0 + approach.s * (z1 - z0);
                    let zd = level0 + approach.t * (level1 - level0);
                    let gap = (zd - zu).abs() - radius - design.half_width;
                    result.vertical_gap = Some(gap);
                    result.vertical_tolerance = tolerance.vertical;
                    if result.status != ClearanceStatus::Clear {
                        let by_level = against(gap, vertical, requirement.vertical);
                        if by_level > result.status {
                            result.status = by_level;
                            result.note = String::from(if by_level == ClearanceStatus::Clear {
                                "cleared by vertical separation"
                            } else {
                                "vertical separation clears it at the drawn levels, not across \
                                 the tolerance"
                            });
                        }
                    }
                }
                if radius == 0.0 && result.note.is_empty() {
                    result.note =
                        String::from("no diameter recorded; the gap is to the centre line");
                }

                if governing
                    .as_ref()
                    .is_none_or(|current| governs(&result, current))
                {
                    governing = Some(result);
                }
            }
            // The design has two vertices or more, so one result at least.
            if let Some(governing) = governing {
                results.push(governing);
            }
        }
    }
    Ok(results)
}

/// The value a delivery schema's Clash attribute should take (TfNSW's is one of
/// No, Hard, Soft, Unknown), worst first. Hard: the service and the works
/// overlap at the drawn position, in plan and, where levels say, in level.
/// Soft: they do not touch but the required clearance is not met, or is met
/// only at the drawn position. Unknown: the service is QL-C or QL-D and near.
/// A SUGGESTION for the attribute, from geometry alone - the schema's clash is
/// the designer's call.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Clash {
    Hard,
    Soft,
    Unknown,
    No,
}

/// "Hard", "Soft", "Unknown", "No".
impl Named for Clash {
    fn name(self) -> &'static str {
        match self {
            Clash::Hard => "Hard",
            Clash::Soft => "Soft",
            Clash::Unknown => "Unknown",
            Clash::No => "No",
        }
    }
}

pub fn clash_of(result: &ClearanceResult) -> Clash {
    match result.status {
        ClearanceStatus::Clear => return Clash::No,
        ClearanceStatus::Unconfirmed => return Clash::Unknown,
        ClearanceStatus::WithinTolerance => return Clash::Soft,
        ClearanceStatus::Conflict => {}
    }
    let overlap_in_plan = result.horizontal_gap < 0.0;
    let overlap_in_level = result.vertical_gap.is_none_or(|gap| gap < 0.0);
    if overlap_in_plan && overlap_in_level {
        Clash::Hard
    } else {
        Clash::Soft
    }
}
