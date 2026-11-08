//! Corridor cross sections and earthwork quantities along an alignment
//! (include/katana/cad/corridor.hpp, src/katana_cad/corridor.cpp; PLAN.MD
//! Phase 21).
//!
//! A corridor is what a horizontal alignment, a design profile and a template
//! make together: at every station the template is set out across the
//! centreline at the design elevation, its outer edges are carried to the
//! ground by batter slopes, and the area between the finished shape and the
//! ground is what has to be dug out or filled in. Summed along the alignment
//! by the end-area method, that is the earthwork quantity - the number an
//! earthworks estimate, a tender and a claim are all built on.
//!
//! THE ASSEMBLY IS THE SIMPLE CASE OF A ROAD TEMPLATE. One carriageway of
//! `half_width` each side of the centreline, falling away at `crossfall`, with
//! a cut batter and a fill batter to daylight. Kerbs, verges, benches and
//! pavement courses are the road templates' (road::road_template), and these
//! functions are that one engine (road::road_section) run on
//! road::road_template::template_from_assembly - kept for the callers that
//! have an assembly, so a quantity from either cannot come to differ.
//!
//! AVERAGE END AREA, and why. The volume between two stations is taken as the
//! mean of their cross-section areas times the distance between them. That is
//! the method every earthworks specification and every measurement standard
//! names (it is the basis of payment in most road contracts), so it is the one
//! a quantity from this software can be compared with. The prismoidal
//! correction is more accurate where areas change quickly and is not applied;
//! a caller wanting it shortens the interval, which is what practice does.
//!
//! GROUND IS QUERIED, NOT SECTIONED. Each cross section asks the surface for
//! its elevation at a set of offsets directly, rather than cutting a section
//! with extract_section and then reading it. A section carries breaks and
//! crossings this does not need, and the daylight search wants the ground at
//! arbitrary offsets rather than at a fixed set of samples.

use katana_core::{Error, ErrorCode, Result, Status};
use katana_geometry::{Point2, SolvedAlignment, SolvedProfile};
use katana_math::cpp;
use katana_math::numerics::tolerance;
use katana_math::summation::CompensatedSum;
use katana_terrain::{TinSurface, signed_trapezoid};

use crate::road::road_section::{RoadSection, road_section, section_stations, sections_surface};
use crate::road::road_template::template_from_assembly;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Assembly {
    /// Metres from the centreline to each edge.
    pub half_width: f64,
    /// Rise/run, positive falls AWAY from the centreline.
    pub crossfall: f64,
    /// Batter slopes as horizontal run per unit of vertical rise: 2.0 is
    /// "2:1", two metres out for every metre up or down. Cut batters are
    /// usually steeper than fill ones, which is why there are two.
    pub cut_batter: f64,
    pub fill_batter: f64,
    /// How far beyond the edge the daylight search will look before giving
    /// up. A batter that has not met the ground within this is reported as a
    /// station with no daylight rather than extended indefinitely.
    pub maximum_batter_width: f64,
}

impl Default for Assembly {
    fn default() -> Self {
        Self {
            half_width: 3.5,
            crossfall: 0.025,
            cut_batter: 1.5,
            fill_batter: 2.0,
            maximum_batter_width: 50.0,
        }
    }
}

/// One vertex of a cross section: offset from the centreline (positive left,
/// looking along increasing station) and elevation.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SectionVertex {
    pub offset: f64,
    pub elevation: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CorridorSection {
    pub station: f64,
    /// World position of the centreline.
    pub centre: Point2,
    /// From the profile.
    pub design_elevation: f64,
    /// The finished shape from left daylight to right daylight, and the
    /// ground it sits on over the same offsets. Both left to right.
    pub design: Vec<SectionVertex>,
    pub ground: Vec<SectionVertex>,
    /// Ground above design, square metres (>= 0).
    pub cut_area: f64,
    /// Design above ground (>= 0).
    pub fill_area: f64,
    /// False when a batter failed to meet the ground within
    /// Assembly::maximum_batter_width, or the ground has no elevation at an
    /// offset the section needs. Such a station contributes no area, and the
    /// count of them is reported so that a quantity with holes in it says so.
    pub complete: bool,
}

impl Default for CorridorSection {
    fn default() -> Self {
        Self {
            station: 0.0,
            centre: Point2::default(),
            design_elevation: 0.0,
            design: Vec::new(),
            ground: Vec::new(),
            cut_area: 0.0,
            fill_area: 0.0,
            complete: true,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CorridorQuantities {
    /// Ascending station.
    pub sections: Vec<CorridorSection>,
    /// Cubic metres.
    pub cut: f64,
    pub fill: f64,
    /// fill - cut: positive means material must be brought in.
    pub net: f64,
    pub incomplete_sections: usize,
    /// The station range actually quantified: the overlap of the alignment,
    /// the profile and the requested range.
    pub start_station: f64,
    pub end_station: f64,
}

/// The finished design as a surface: the template strings of every complete
/// section, triangulated with the five longitudinal strings - both daylights,
/// both edges, the centreline - as breaklines, and the daylight lines as the
/// boundary so nothing is hulled across the inside of a curve. What the 3D
/// view shows, and what compare_surfaces can measure against the ground: an
/// independent check on the end-area quantities, which the tests make.
///
/// Sections that cannot reach the ground are left out and the strings broken
/// there. When any are, the boundary is not used - a ring with gaps in it is
/// not a ring - and the surface is bounded by its convex hull instead, which
/// can then reach across a gap; the count says when that has happened.
///
/// Where two strings cross in plan, on the inside of a curve tighter than the
/// corridor is wide, the crossing takes the mean of their elevations. That is
/// a compromise: the true design there is undefined, and refusing the whole
/// surface for it would lose the 99% that is fine.
#[derive(Clone, Debug, Default)]
pub struct CorridorSurface {
    pub surface: TinSurface,
    /// That went into the surface.
    pub sections: usize,
    /// That were left out.
    pub incomplete_sections: usize,
}

/// The area between two piecewise-linear lines over a common offset range,
/// split into the part where `upper` is above `lower` and the part where it
/// is below.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SplitArea {
    pub above: f64,
    pub below: f64,
}

fn validate_assembly(assembly: &Assembly) -> Status {
    let finite = assembly.half_width.is_finite()
        && assembly.crossfall.is_finite()
        && assembly.cut_batter.is_finite()
        && assembly.fill_batter.is_finite()
        && assembly.maximum_batter_width.is_finite();
    if !finite {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the assembly has a non-finite value",
        ));
    }
    if !(assembly.half_width > 0.0) {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the assembly half width must be positive",
        ));
    }
    if !(assembly.cut_batter > 0.0) || !(assembly.fill_batter > 0.0) {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "batter slopes must be positive (horizontal run per unit rise)",
        ));
    }
    if !(assembly.maximum_batter_width > 0.0) {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the maximum batter width must be positive",
        ));
    }
    Ok(())
}

// Linear interpolation on a line whose vertices are ordered by offset, either
// direction; None outside its range.
fn elevation_on(line: &[SectionVertex], offset: f64) -> Option<f64> {
    if line.len() < 2 {
        return None;
    }
    for pair in line.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        let lo = cpp::min(a.offset, b.offset);
        let hi = cpp::max(a.offset, b.offset);
        if offset < lo - 1e-12 || offset > hi + 1e-12 {
            continue;
        }
        if hi - lo < 1e-12 {
            return Some(a.elevation);
        }
        let t = (offset - a.offset) / (b.offset - a.offset);
        return Some(a.elevation + (b.elevation - a.elevation) * t);
    }
    None
}

/// Returns {above, below}. Exposed because it is where the arithmetic is, and
/// worth testing on its own: both lines are sampled at the union of their
/// vertices and at every crossing, so each piece is a trapezoid of one sign.
pub fn area_between(upper: &[SectionVertex], lower: &[SectionVertex]) -> SplitArea {
    let mut result = SplitArea::default();
    if upper.len() < 2 || lower.len() < 2 {
        return result;
    }
    let mut offsets: Vec<f64> = Vec::with_capacity(upper.len() + lower.len());
    offsets.extend(upper.iter().map(|v| v.offset));
    offsets.extend(lower.iter().map(|v| v.offset));
    offsets.sort_by(f64::total_cmp);
    offsets.dedup_by(|b, a| (*a - *b).abs() < 1e-12);

    let mut above = CompensatedSum::default();
    let mut below = CompensatedSum::default();
    for pair in offsets.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let (Some(ua), Some(ub), Some(la), Some(lb)) = (
            elevation_on(upper, a),
            elevation_on(upper, b),
            elevation_on(lower, a),
            elevation_on(lower, b),
        ) else {
            continue; // one line does not cover this interval
        };
        // The one sign split of a linear strip, the volumes' too: where the
        // lines cross inside the interval, two triangles of opposite sign. A
        // part of nothing adds nothing to a compensated sum, so this is the
        // arithmetic it was, to the bit.
        let area = signed_trapezoid(ua - la, ub - lb, b - a);
        above.add(area.positive);
        below.add(area.negative);
    }
    result.above = above.value();
    result.below = below.value();
    result
}

// The engine's section in the old shape: the finished line from daylight to
// daylight once both batters met the ground, the ground under it when it was
// found everywhere, and the areas when both were.
fn old_section(section: &RoadSection) -> CorridorSection {
    let mut out = CorridorSection {
        station: section.station,
        centre: section.centre,
        design_elevation: section.design_level,
        complete: section.complete,
        ..CorridorSection::default()
    };
    if section.left_reached && section.right_reached {
        out.design = section
            .top
            .iter()
            .map(|point| SectionVertex {
                offset: point.offset,
                elevation: point.level,
            })
            .collect();
    }
    out.ground = section.ground.clone();
    out.cut_area = section.cut_area;
    out.fill_area = section.fill_area;
    out
}

// The sections corridor_quantities and corridor_surface are made from, with
// their refusals: the assembly, the interval, and an alignment and profile
// that do not overlap. Also the station range, as (start, end).
fn assembly_sections(
    alignment: &SolvedAlignment,
    profile: &SolvedProfile,
    assembly: &Assembly,
    ground: &TinSurface,
    interval: f64,
) -> Result<(Vec<RoadSection>, f64, f64)> {
    validate_assembly(assembly)?;
    if !(interval > 0.0) || !interval.is_finite() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the interval must be positive",
        ));
    }
    let start = cpp::max(alignment.start_station(), profile.start_station());
    let end = cpp::min(alignment.end_station(), profile.end_station());
    if !(end - start > tolerance::COORDINATE) {
        return Err(Error::new(
            ErrorCode::InvalidGeometry,
            "the alignment and the design profile do not overlap in station",
        ));
    }
    // Every interval station, and every key station of the alignment and the
    // profile inside the range - the quantities change character at a TS, a
    // PVC and a PVT, and a report that stepped over them would be one nobody
    // could check.
    let mut keys = profile.key_stations();
    keys.extend(alignment.key_stations());
    let road_template = template_from_assembly(assembly);
    let mut sections = Vec::new();
    for station in section_stations(start, end, interval, &keys) {
        sections.push(road_section(
            &road_template,
            alignment,
            profile,
            Some(ground),
            station,
            None,
        )?);
    }
    Ok((sections, start, end))
}

/// The cross section at one station: one engine (road::road_section), the
/// assembly a template like any other (template_from_assembly), checked first
/// in its own words. Fails with InvalidArgument when the station is outside
/// the alignment or the profile, or the assembly is not sensible
/// (non-positive width or batters, non-finite anything).
pub fn corridor_section(
    alignment: &SolvedAlignment,
    profile: &SolvedProfile,
    assembly: &Assembly,
    ground: &TinSurface,
    station: f64,
) -> Result<CorridorSection> {
    validate_assembly(assembly)?;
    let section = road_section(
        &template_from_assembly(assembly),
        alignment,
        profile,
        Some(ground),
        station,
        None,
    )?;
    Ok(old_section(&section))
}

/// Sections at `interval` along the part of the alignment the profile covers,
/// always including the alignment's and the profile's key stations and both
/// ends of the range (road::road_section::section_stations), and the end-area
/// volumes between them. Fails with InvalidArgument for a non-positive
/// interval or an assembly that corridor_section rejects, and with
/// InvalidGeometry when the alignment and the profile do not overlap at all.
pub fn corridor_quantities(
    alignment: &SolvedAlignment,
    profile: &SolvedProfile,
    assembly: &Assembly,
    ground: &TinSurface,
    interval: f64,
) -> Result<CorridorQuantities> {
    let (sections, start, end) = assembly_sections(alignment, profile, assembly, ground, interval)?;
    let mut result = CorridorQuantities {
        start_station: start,
        end_station: end,
        ..CorridorQuantities::default()
    };
    result.sections.reserve(sections.len());
    for section in &sections {
        if !section.complete {
            result.incomplete_sections += 1;
        }
        result.sections.push(old_section(section));
    }
    // Average end area between consecutive complete sections. An interval
    // touching an incomplete section contributes nothing; the count of those
    // sections is in the result so the total is never mistaken for whole.
    let mut cut = CompensatedSum::default();
    let mut fill = CompensatedSum::default();
    for pair in result.sections.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        if !a.complete || !b.complete {
            continue;
        }
        let length = b.station - a.station;
        cut.add(0.5 * (a.cut_area + b.cut_area) * length);
        fill.add(0.5 * (a.fill_area + b.fill_area) * length);
    }
    result.cut = cut.value();
    result.fill = fill.value();
    result.net = result.fill - result.cut;
    Ok(result)
}

/// Fails as corridor_quantities does; with InvalidGeometry when fewer than
/// two sections are complete; and with whatever terrain's build_tin reports.
pub fn corridor_surface(
    alignment: &SolvedAlignment,
    profile: &SolvedProfile,
    assembly: &Assembly,
    ground: &TinSurface,
    interval: f64,
) -> Result<CorridorSurface> {
    let (sections, _, _) = assembly_sections(alignment, profile, assembly, ground, interval)?;
    let pointers: Vec<&RoadSection> = sections.iter().collect();
    // Complete sections only, the strings broken where one is left out and no
    // boundary then, as this function always built it.
    let surface = sections_surface(&pointers, alignment, false, true)?;
    Ok(CorridorSurface {
        surface: surface.surface,
        sections: surface.sections,
        incomplete_sections: surface.left_out,
    })
}
