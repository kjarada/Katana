//! The corridor engine (include/katana/cad/road/road_section.hpp,
//! src/katana_cad/road/road_section.cpp; docs/road_design.md, "Building a
//! corridor"): a road template set across an alignment at the design level,
//! section by section, and what is made from the sections - the earthworks,
//! the strings, the finished and subgrade surfaces, and the 3D objects of the
//! road model.
//!
//! ONE ENGINE. The old one-carriageway functions (corridor.rs:
//! corridor_section, corridor_quantities, corridor_surface) are this engine run
//! on template_from_assembly, so a quantity from the old window dialog and one
//! from CORRIDOR QUANTITIES cannot come to differ.
//!
//! A SECTION, side by side. Out from the centreline at the design level, each
//! component of a side in turn: a lane or a shoulder (its width at its
//! crossfall, or at the superelevation table's for the station when it is
//! superelevated), a kerb (its shape, road_template.rs), a table drain (a V of
//! its width, its invert `depth` below its inner edge), a footpath, a verge, a
//! median - and last a batter, marched out from the hinge (the last top point)
//! to the target surface as the old corridor did: half-metre steps until the
//! batter and the ground change order, then forty bisections. Its benches are
//! flats of their width every bench height. A batter that does not meet the
//! ground within its maximum width, or meets no ground at all, leaves its side
//! NOT REACHED and the section incomplete.
//!
//! THE FORMATION (the subgrade) is the bottom of each component's courses, a
//! kerb's base, the top of what has no courses, then the batter. Where two
//! neighbours' bottoms differ, or the courses end at the hinge, the line
//! steps: the shallower corner is set FACE_RUN into its own component, so the
//! line stays a function of offset - which a TIN of it, and the end areas,
//! need. A side of a batter alone starts its line at the hinge, the
//! centreline.
//!
//! EARTHWORKS are between the formation and the ground across the section:
//! cut where the ground is above, fill where it is below - the pavement box is
//! cut, the courses sit on the fill - by the end-area method between two
//! complete sections, as corridor_quantities always measured them.

use std::collections::BTreeMap;
use std::sync::Arc;

use katana_core::error::make_error;
use katana_core::text::format_exact_real;
use katana_core::{Error, ErrorCode, Result};
use katana_entity::{Color, EnumName};
use katana_geometry::alignment::solve_alignment;
use katana_geometry::mesh::mesh_volume;
use katana_geometry::profile::solve_profile;
use katana_geometry::{
    AlignmentPi, AlignmentShape, HorizontalAlignment, Point2, Point3, Polyline2, ProfilePvi,
    SolvedAlignment, SolvedProfile, TriangleMesh, VerticalAlignment,
};
use katana_math::cpp;
use katana_math::numerics::tolerance;
use katana_math::summation::CompensatedSum;
use katana_terrain::{
    Breakline, CrossingBreaklinePolicy, TinBuildOptions, TinInput, TinSurface, build_tin,
};

use crate::bim::bim_object::{BimModel, BimObject, BimProblem, Quantity};
use crate::corridor::{SectionVertex, area_between};
use crate::document::Document;
use crate::road::corridor_model::{Corridor, Crossfalls, check_corridor, superelevation_at};
use crate::road::road_template::{
    Component, ComponentKind, KerbFacing, KerbShape, MAXIMUM_BENCHES, RoadTemplate,
    RoadTemplateSet, ShapeColumn, Side, check_template, kerb_shape, kerb_width,
};

/// A vertical face of the formation is given this much run, so the line is a
/// function of offset: more than the TIN builder's duplicate tolerance
/// (math::tolerance::COORDINATE, 0.1 mm), so a TIN keeps both corners, and
/// less than any construction tolerance on a subgrade. The area it moves is
/// under a thousandth of a square metre for a metre-deep pavement.
pub const FACE_RUN: f64 = 0.001;
/// The columns an earthworks solid has across each side - its look, not its
/// quantity, which is the end area of the true lines: spread over the stretch
/// of the side where it has earthworks, every 2.5 % of it.
pub const EARTHWORKS_COLUMNS: usize = 41;

// More sections than this is refused before one is made: a millimetre
// interval typed for a metre one, on a long road, would otherwise take the
// session with it. ALIGN STATIONS draws the same line (docs/cad.md).
const MAXIMUM_SECTIONS: usize = 100000;

// The daylight march: half a metre resolves any batter a machine can build,
// and forty bisections put the crossing at 5e-13 m (docs/cad.md, "Corridor
// quantities"). The ground is sampled at the same spacing across a section.
const STEP: f64 = 0.5;
const BISECTIONS: i32 = 40;

fn number(value: f64) -> String {
    format_exact_real(value)
}

/// A point of a section's line: what it is ("CL", "left/lane/outer",
/// "right/batter/daylight"), its offset (positive left) and its level.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SectionPoint {
    pub code: String,
    pub offset: f64,
    pub level: f64,
}

/// A stretch of one side, as distances OUT from the centreline (from < to),
/// over which the earthworks have thickness.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct OutSpan {
    pub from: f64,
    pub to: f64,
}

/// One vertical of a solid in a section: offset, top level, bottom level.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SolidColumn {
    pub offset: f64,
    pub top: f64,
    pub bottom: f64,
}

/// A course or a kerb in one section. Every section of a template has the
/// same solids with the same columns, so a run of them lofts into one closed
/// solid (loft_solid).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SectionSolid {
    /// "left/lane/AC14", "left/kerb".
    pub key: String,
    /// In RoadTemplate::components.
    pub component: usize,
    /// In its courses; none for a kerb.
    pub course: Option<usize>,
    /// Outward from the centreline, at least two.
    pub columns: Vec<SolidColumn>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RoadSection {
    /// The run of its corridor it belongs to (CorridorRun::region, from 1); 0
    /// for a section made on its own. Where two runs meet, each has a section
    /// at the one station, and this is what tells them apart.
    pub run: usize,
    pub station: f64,
    pub centre: Point2,
    /// Of the alignment, radians from +x.
    pub direction: f64,
    pub design_level: f64,
    /// Left outermost to right outermost: the daylights where the batters met
    /// the ground, else the hinges.
    pub top: Vec<SectionPoint>,
    pub formation: Vec<SectionPoint>,
    pub solids: Vec<SectionSolid>,
    /// The ground across the section, left to right: at every vertex of the
    /// two lines and every half metre between. Empty when the section is not
    /// complete.
    pub ground: Vec<SectionVertex>,
    /// A side is reached when its batter met the ground, or it has no batter.
    pub left_reached: bool,
    pub right_reached: bool,
    /// Both reached and the ground found everywhere across.
    pub complete: bool,
    /// Square metres between the formation and the ground (zero when not
    /// complete): cut where the ground is above it, fill where below, each
    /// side's split at the centreline.
    pub cut_area: f64,
    pub fill_area: f64,
    pub left_cut: f64,
    pub left_fill: f64,
    pub right_cut: f64,
    pub right_fill: f64,
    /// Where on each side the ground is above the formation (cut) or below it
    /// (fill) by more than math::tolerance::COORDINATE (0.1 mm) at some point
    /// of the stretch, left to right as the section is: the stretches the
    /// earthworks occupy in plan. A side with none has no cut (or fill) here,
    /// and its area above is exactly zero: the daylight is found to about
    /// 1e-13 m, so it lands on the wrong side of the ground half the time, and
    /// that sliver of 1e-26 m2 is not earthworks. One place decides what
    /// "none" means (build_section); the objects and the quantities follow
    /// it. Empty when the section is not complete.
    pub left_cut_spans: Vec<OutSpan>,
    pub left_fill_spans: Vec<OutSpan>,
    pub right_cut_spans: Vec<OutSpan>,
    pub right_fill_spans: Vec<OutSpan>,
}

// A point of one side, by its distance OUT from the centreline.
#[derive(Clone, Debug, Default)]
struct OutPoint {
    code: String,
    out: f64,
    level: f64,
}

struct OutSolid {
    key: String,
    component: usize,
    course: Option<usize>,
    // `out` from the centreline, absolute levels.
    columns: Vec<ShapeColumn>,
}

#[derive(Default)]
struct SideBuild {
    // From the centreline outward.
    top: Vec<OutPoint>,
    // From out 0 outward.
    formation: Vec<OutPoint>,
    solids: Vec<OutSolid>,
    reached: bool,
}

// Appends a component's first formation point where the line already ends at
// that distance out: one point when the levels agree, else the shallower
// corner stepped FACE_RUN into its own component (room: a quarter of each
// component's width, for a component narrower than the step).
fn join_formation(
    line: &mut Vec<OutPoint>,
    mut point: OutPoint,
    previous_room: f64,
    current_room: f64,
) {
    if line.is_empty() {
        line.push(point);
        return;
    }
    let at = line.len() - 1;
    let last = &mut line[at];
    if (last.out - point.out).abs() > tolerance::GEOMETRIC
        || (last.level - point.level).abs() <= tolerance::COORDINATE
    {
        if (last.out - point.out).abs() > tolerance::GEOMETRIC {
            line.push(point);
        }
        return;
    }
    let step = cpp::min3(FACE_RUN, previous_room / 4.0, current_room / 4.0);
    if last.level > point.level {
        last.out -= step; // the previous component's corner is the shallower
    } else {
        point.out += step;
    }
    line.push(point);
}

// The batter's rise (or fall) `d` out from the hinge: 1 in `run`, with a flat
// of bench_width after every bench_height.
fn batter_rise(batter: &Component, cut: bool, d: f64) -> f64 {
    let run = if cut {
        batter.cut_slope
    } else {
        batter.fill_slope
    };
    if !(batter.bench_height > 0.0) || !(batter.bench_width > 0.0) {
        return d / run;
    }
    let slope = batter.bench_height * run;
    let period = slope + batter.bench_width;
    let cycles = (d / period).floor();
    let within = d - cycles * period;
    cycles * batter.bench_height + cpp::min(within, slope) / run
}

// The ground's level at a signed offset across the section, if it has one.
type GroundAt<'a> = &'a dyn Fn(f64) -> Option<f64>;

fn add_point(out: &mut SideBuild, point: OutPoint) {
    out.top.push(point.clone());
    out.formation.push(point);
}

fn build_side(
    road_template: &RoadTemplate,
    side: Side,
    design_level: f64,
    superelevation: Option<Crossfalls>,
    ground_at: GroundAt<'_>,
    have_ground: bool,
) -> SideBuild {
    let sign = if side == Side::Left { 1.0 } else { -1.0 };
    let side_word = side.name();
    // A component's crossfall at this station: the corridor's superelevation
    // table's for its side when it is superelevated and the table has an
    // entry, else its own. Every component that has a crossfall reads it here
    // - a lane, an edge strip between superelevated lanes and shoulders, a
    // drain.
    let crossfall_of = |component: &Component| -> f64 {
        match superelevation {
            Some(table) if component.superelevated => {
                if side == Side::Left {
                    table.left
                } else {
                    table.right
                }
            }
            _ => component.crossfall,
        }
    };
    let mut out = SideBuild::default();
    out.top.push(OutPoint {
        code: String::from("CL"),
        out: 0.0,
        level: design_level,
    });
    let mut at_out = 0.0;
    let mut at_level = design_level;
    let mut previous_width = 0.0;
    let mut batter: Option<&Component> = None;
    let mut batter_prefix = String::new();

    for (index, component) in road_template.components.iter().enumerate() {
        if component.side != side {
            continue;
        }
        let prefix = format!("{side_word}/{}", component.name);
        if component.kind == ComponentKind::Batter {
            batter = Some(component);
            batter_prefix = prefix;
            continue;
        }
        // The component's top, a column at each change of it, with its
        // bottom; and the names of its points after the first, which is where
        // the component before it ended.
        let (surface, codes): (Vec<ShapeColumn>, Vec<&str>) =
            if component.kind == ComponentKind::Kerb {
                // Only an edge strip has a crossfall (check_template refuses a
                // superelevated barrier, mountable or kerb-and-gutter kerb).
                let mut shaped = component.clone();
                shaped.crossfall = crossfall_of(component);
                let surface: Vec<ShapeColumn> = kerb_shape(&shaped)
                    .iter()
                    .map(|column| ShapeColumn {
                        out: at_out + column.out,
                        top: at_level + column.top,
                        bottom: at_level + column.bottom,
                    })
                    .collect();
                // The points after the first, which is where the previous component
                // ended. A kerb facing outward is the other way round: its top
                // first (the face's crest), then down the face to the lip.
                let outward = component.faces == KerbFacing::Outward;
                let codes = match component.shape {
                    KerbShape::Barrier | KerbShape::Mountable => {
                        if outward {
                            vec!["face", "lip"]
                        } else {
                            vec!["face", "back"]
                        }
                    }
                    KerbShape::KerbAndGutter => {
                        if outward {
                            vec!["face", "invert", "lip"]
                        } else {
                            vec!["invert", "face", "back"]
                        }
                    }
                    KerbShape::EdgeStrip => vec!["outer"],
                };
                out.solids.push(OutSolid {
                    key: prefix.clone(),
                    component: index,
                    course: None,
                    columns: surface.clone(),
                });
                (surface, codes)
            } else {
                let (mut surface, codes) = if component.kind == ComponentKind::Drain {
                    let mut surface = vec![
                        ShapeColumn {
                            out: at_out,
                            top: at_level,
                            bottom: at_level,
                        },
                        ShapeColumn {
                            out: at_out + component.width / 2.0,
                            top: at_level - component.depth,
                            bottom: at_level - component.depth,
                        },
                        ShapeColumn {
                            out: at_out + component.width,
                            top: at_level + component.width * crossfall_of(component) / 100.0,
                            bottom: 0.0,
                        },
                    ];
                    surface[2].bottom = surface[2].top;
                    (surface, vec!["invert", "outer"])
                } else {
                    let outer = at_level + component.width * crossfall_of(component) / 100.0;
                    (
                        vec![
                            ShapeColumn {
                                out: at_out,
                                top: at_level,
                                bottom: at_level,
                            },
                            ShapeColumn {
                                out: at_out + component.width,
                                top: outer,
                                bottom: outer,
                            },
                        ],
                        vec!["outer"],
                    )
                };
                let mut depth = 0.0;
                for (c, course) in component.courses.iter().enumerate() {
                    let solid = OutSolid {
                        key: format!("{prefix}/{}", course.name),
                        component: index,
                        course: Some(c),
                        columns: surface
                            .iter()
                            .map(|column| ShapeColumn {
                                out: column.out,
                                top: column.top - depth,
                                bottom: column.top - depth - course.thickness,
                            })
                            .collect(),
                    };
                    depth += course.thickness;
                    out.solids.push(solid);
                }
                for column in &mut surface {
                    column.bottom = column.top - depth;
                }
                (surface, codes)
            };

        for (column, code) in surface.iter().skip(1).zip(&codes) {
            out.top.push(OutPoint {
                code: format!("{prefix}/{code}"),
                out: column.out,
                level: column.top,
            });
        }
        let last = surface.len() - 1;
        let width = surface[last].out - surface[0].out;
        for (j, column) in surface.iter().enumerate() {
            let point = OutPoint {
                code: format!("{prefix}/f{j}"),
                out: column.out,
                level: column.bottom,
            };
            if j == 0 {
                join_formation(&mut out.formation, point, previous_width, width);
            } else {
                out.formation.push(point);
            }
        }
        previous_width = width;
        at_out = surface[last].out;
        at_level = surface[last].top;
    }

    let Some(batter) = batter else {
        // Nothing to carry to the ground: the side ends at its edge.
        out.reached = true;
        return out;
    };
    let hinge = out.top[out.top.len() - 1].clone();
    // The formation comes up to the hinge, where the batter starts: the
    // courses' outer bottom corner stepped FACE_RUN back into its component.
    if !out.formation.is_empty() {
        let at = out.formation.len() - 1;
        let corner = &mut out.formation[at];
        if (corner.out - hinge.out).abs() <= tolerance::GEOMETRIC
            && (corner.level - hinge.level).abs() > tolerance::COORDINATE
        {
            corner.out -= cpp::min(FACE_RUN, previous_width / 4.0);
            out.formation.push(hinge.clone());
        }
    } else {
        // A side of a batter alone (a ramp's edge, an embankment of nothing
        // but its slopes): nothing has put a point on the formation, and the
        // batter would start at its first bench or its daylight, so the line
        // would run straight from there to the other side's first point and
        // lose the triangle under the batter. The hinge - here the centreline
        // at the design level - is where it starts. build_section joins it
        // with the other side's first point at the centreline.
        out.formation.push(OutPoint {
            code: format!("{batter_prefix}/hinge"),
            out: hinge.out,
            level: hinge.level,
        });
    }
    if !have_ground {
        return out;
    }
    let Some(ground_at_hinge) = ground_at(sign * hinge.out) else {
        return out;
    };
    let difference = ground_at_hinge - hinge.level;
    if difference.abs() <= tolerance::COORDINATE {
        // At grade.
        add_point(
            &mut out,
            OutPoint {
                code: format!("{batter_prefix}/daylight"),
                out: hinge.out,
                level: hinge.level,
            },
        );
        out.reached = true;
        return out;
    }
    // In cut the batter climbs from the hinge to the ground; in fill it falls
    // to it. "Crossed": the batter has passed the ground.
    let cut = difference > 0.0;
    let towards = if cut { 1.0 } else { -1.0 };
    let level_at = |d: f64| hinge.level + towards * batter_rise(batter, cut, d);
    let crossed = |d: f64| -> Option<bool> {
        let z = ground_at(sign * (hinge.out + d))?;
        let level = level_at(d);
        Some(if cut { level >= z } else { level <= z })
    };
    let mut reach: Option<f64> = None;
    let mut previous = 0.0;
    // Half-metre steps, each counted from the hinge (k x STEP: a sum of steps
    // drifts), and the last one the limit itself: a limit that is not a
    // multiple of a step, or is under one step, is tried at the limit, not
    // given up on short of it.
    let mut k: usize = 1;
    loop {
        let d = cpp::min(k as f64 * STEP, batter.maximum_width);
        let Some(at) = crossed(d) else {
            return out;
        };
        if at {
            let mut low = previous;
            let mut high = d;
            for _ in 0..BISECTIONS {
                let mid = 0.5 * (low + high);
                let Some(there) = crossed(mid) else {
                    return out;
                };
                if there {
                    high = mid;
                } else {
                    low = mid;
                }
            }
            reach = Some(0.5 * (low + high));
            break;
        }
        previous = d;
        if d >= batter.maximum_width {
            break;
        }
        k += 1;
    }
    let Some(reach) = reach else {
        return out;
    };
    if batter.bench_height > 0.0 && batter.bench_width > 0.0 {
        let run = if cut {
            batter.cut_slope
        } else {
            batter.fill_slope
        };
        let slope = batter.bench_height * run;
        // check_template refuses a batter with more than MAXIMUM_BENCHES; the
        // bound here is the loop's own, so no template can make it long.
        let mut bench: usize = 1;
        let mut d = slope;
        while d < reach && bench <= MAXIMUM_BENCHES {
            add_point(
                &mut out,
                OutPoint {
                    code: format!("{batter_prefix}/bench{bench}-in"),
                    out: hinge.out + d,
                    level: level_at(d),
                },
            );
            let bench_end = d + batter.bench_width;
            if bench_end >= reach {
                break;
            }
            add_point(
                &mut out,
                OutPoint {
                    code: format!("{batter_prefix}/bench{bench}-out"),
                    out: hinge.out + bench_end,
                    level: level_at(bench_end),
                },
            );
            d += slope + batter.bench_width;
            bench += 1;
        }
    }
    add_point(
        &mut out,
        OutPoint {
            code: format!("{batter_prefix}/daylight"),
            out: hinge.out + reach,
            level: level_at(reach),
        },
    );
    out.reached = true;
    out
}

// The part of a line, ordered by offset either way, inside [lo, hi], with
// points made where it crosses either end.
fn clipped(line: &[SectionVertex], lo: f64, hi: f64) -> Vec<SectionVertex> {
    let mut out: Vec<SectionVertex> = Vec::new();
    let mut add = |offset: f64, elevation: f64| {
        if out
            .last()
            .is_none_or(|last| (last.offset - offset).abs() > tolerance::ABSOLUTE)
        {
            out.push(SectionVertex { offset, elevation });
        }
    };
    for pair in line.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let from = cpp::min(a.offset, b.offset);
        let to = cpp::max(a.offset, b.offset);
        let s = cpp::max(from, lo);
        let e = cpp::min(to, hi);
        if s > e {
            continue;
        }
        let at = |offset: f64| {
            if to - from <= tolerance::ABSOLUTE {
                a.elevation
            } else {
                a.elevation
                    + (b.elevation - a.elevation) * (offset - a.offset) / (b.offset - a.offset)
            }
        };
        // In the line's own direction.
        if a.offset <= b.offset {
            add(s, at(s));
            add(e, at(e));
        } else {
            add(e, at(e));
            add(s, at(s));
        }
    }
    out
}

// A line's level at an offset, the ends held beyond its span.
fn level_on(line: &[SectionVertex], offset: f64) -> f64 {
    let Some(first) = line.first() else {
        return 0.0;
    };
    let mut best = first.elevation;
    let mut nearest = (first.offset - offset).abs();
    for pair in line.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let from = cpp::min(a.offset, b.offset);
        let to = cpp::max(a.offset, b.offset);
        if offset >= from && offset <= to {
            if to - from <= tolerance::ABSOLUTE {
                return a.elevation;
            }
            return a.elevation
                + (b.elevation - a.elevation) * (offset - a.offset) / (b.offset - a.offset);
        }
    }
    for vertex in line {
        let distance = (vertex.offset - offset).abs();
        if distance < nearest {
            nearest = distance;
            best = vertex.elevation;
        }
    }
    best
}

fn vertices(points: &[SectionPoint]) -> Vec<SectionVertex> {
    points
        .iter()
        .map(|point| SectionVertex {
            offset: point.offset,
            elevation: point.level,
        })
        .collect()
}

// The stretches of one side - distances OUT from the centreline, ascending -
// where the ground is above the formation (`cutting`) or below it, thick
// somewhere by more than COORDINATE. `formation_levels[i]` is the formation's
// level under `ground[i]`. The ground samples include every vertex of the
// formation, so between two samples both lines are straight and their
// difference crosses zero at one point, found exactly. A stretch whose
// thickest point is under COORDINATE (0.1 mm, the TIN's duplicate tolerance)
// is not earthworks: the daylight is bisected to about 1e-13 m, and lands on
// either side of the ground by that much.
fn earthwork_spans(
    ground: &[SectionVertex],
    formation_levels: &[f64],
    side: Side,
    cutting: bool,
) -> Vec<OutSpan> {
    #[derive(Clone, Copy)]
    struct Sample {
        out: f64,
        thickness: f64,
    }
    let sign = if side == Side::Left { 1.0 } else { -1.0 };
    let mut samples: Vec<Sample> = Vec::new();
    for (sample, &formation) in ground.iter().zip(formation_levels) {
        let out = sign * sample.offset;
        if out < -tolerance::ABSOLUTE {
            continue; // the other side of the centreline
        }
        let thickness = if cutting {
            sample.elevation - formation
        } else {
            formation - sample.elevation
        };
        samples.push(Sample {
            out: cpp::max(out, 0.0),
            thickness,
        });
    }
    samples.sort_by(|a, b| a.out.total_cmp(&b.out));
    // Where the thickness is zero between two samples of opposite sign (or
    // one of them zero).
    let crossing = |a: Sample, b: Sample| {
        a.out + (b.out - a.out) * (a.thickness / (a.thickness - b.thickness))
    };
    let mut spans = Vec::new();
    let mut open = false;
    let mut start = 0.0;
    let mut peak = 0.0;
    let mut previous: Option<Sample> = None;
    for &sample in &samples {
        if sample.thickness > 0.0 {
            if !open {
                start = match previous {
                    None => sample.out,
                    Some(before) => crossing(before, sample),
                };
                open = true;
                peak = 0.0;
            }
            peak = cpp::max(peak, sample.thickness);
        } else if open {
            // A span is open only after a sample before this one.
            let end = previous.map_or(sample.out, |before| crossing(before, sample));
            if peak > tolerance::COORDINATE && end > start {
                spans.push(OutSpan {
                    from: start,
                    to: end,
                });
            }
            open = false;
        }
        previous = Some(sample);
    }
    if open && let Some(last) = samples.last() {
        let end = last.out;
        if peak > tolerance::COORDINATE && end > start {
            spans.push(OutSpan {
                from: start,
                to: end,
            });
        }
    }
    spans
}

// What a side with no earthworks stretch is left with: no area, whatever
// sliver the daylight's last 1e-13 m left.
fn settle(spans: &[OutSpan], area: &mut f64, trimmed: &mut bool) {
    if spans.is_empty() && *area != 0.0 {
        *area = 0.0;
        *trimmed = true;
    }
}

fn build_section(
    road_template: &RoadTemplate,
    alignment: &SolvedAlignment,
    profile: &SolvedProfile,
    ground: Option<&TinSurface>,
    station: f64,
    superelevation: Option<Crossfalls>,
) -> Result<RoadSection> {
    let centre = alignment.point_at_station(station);
    let direction = alignment.direction_at_station(station);
    let elevation = profile.elevation_at(station);
    let (Some(centre), Some(direction)) = (centre, direction) else {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "the station is outside the alignment",
            number(station),
        ));
    };
    let Some(elevation) = elevation else {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "the station is outside the design profile",
            number(station),
        ));
    };
    let mut section = RoadSection {
        station,
        centre,
        direction,
        design_level: elevation,
        ..RoadSection::default()
    };

    let sine = direction.sin();
    let cosine = direction.cos();
    let ground_at = |offset: f64| -> Option<f64> {
        ground?.elevation_at(Point2::new(
            centre.x - sine * offset,
            centre.y + cosine * offset,
        ))
    };
    let left = build_side(
        road_template,
        Side::Left,
        elevation,
        superelevation,
        &ground_at,
        ground.is_some(),
    );
    let right = build_side(
        road_template,
        Side::Right,
        elevation,
        superelevation,
        &ground_at,
        ground.is_some(),
    );
    section.left_reached = left.reached;
    section.right_reached = right.reached;

    // Left outermost to right outermost; the centreline once.
    for point in left.top.iter().rev() {
        section.top.push(SectionPoint {
            code: point.code.clone(),
            offset: point.out,
            level: point.level,
        });
    }
    for point in right.top.iter().skip(1) {
        section.top.push(SectionPoint {
            code: point.code.clone(),
            offset: -point.out,
            level: point.level,
        });
    }
    // The two formations meet at the centreline, stepped there as between two
    // components when their first courses differ in depth.
    let mut left_formation = left.formation.clone();
    let mut right_formation = right.formation.clone();
    if !left_formation.is_empty()
        && !right_formation.is_empty()
        && left_formation[0].out.abs() <= tolerance::GEOMETRIC
        && right_formation[0].out.abs() <= tolerance::GEOMETRIC
    {
        let left_room = if left_formation.len() > 1 {
            left_formation[1].out - left_formation[0].out
        } else {
            FACE_RUN
        };
        let right_room = if right_formation.len() > 1 {
            right_formation[1].out - right_formation[0].out
        } else {
            FACE_RUN
        };
        let difference = left_formation[0].level - right_formation[0].level;
        if difference.abs() <= tolerance::COORDINATE {
            right_formation.remove(0);
        } else if difference > 0.0 {
            left_formation[0].out += cpp::min(FACE_RUN, left_room / 4.0);
        } else {
            right_formation[0].out += cpp::min(FACE_RUN, right_room / 4.0);
        }
    }
    for point in left_formation.iter().rev() {
        section.formation.push(SectionPoint {
            code: point.code.clone(),
            offset: point.out,
            level: point.level,
        });
    }
    for point in &right_formation {
        section.formation.push(SectionPoint {
            code: point.code.clone(),
            offset: -point.out,
            level: point.level,
        });
    }
    for (built, sign) in [(&left, 1.0), (&right, -1.0)] {
        for solid in &built.solids {
            section.solids.push(SectionSolid {
                key: solid.key.clone(),
                component: solid.component,
                course: solid.course,
                columns: solid
                    .columns
                    .iter()
                    .map(|column| SolidColumn {
                        offset: sign * column.out,
                        top: column.top,
                        bottom: column.bottom,
                    })
                    .collect(),
            });
        }
    }

    if ground.is_none() || !left.reached || !right.reached || section.top.len() < 2 {
        return Ok(section);
    }
    // The ground under the whole span, at every vertex of both lines and
    // every half metre between, so an undulation between two vertices is not
    // straightened out.
    let mut offsets: Vec<f64> = section
        .top
        .iter()
        .chain(&section.formation)
        .map(|point| point.offset)
        .collect();
    let leftmost = section.top[0].offset;
    let rightmost = section.top[section.top.len() - 1].offset;
    let mut o = rightmost;
    while o < leftmost {
        offsets.push(o);
        o += STEP;
    }
    offsets.sort_by(|a, b| b.total_cmp(a)); // left to right
    offsets.dedup_by(|b, a| (*a - *b).abs() < 1e-9);
    let mut sampled = Vec::with_capacity(offsets.len());
    for &offset in &offsets {
        let Some(z) = ground_at(offset) else {
            return Ok(section); // ground missing under the section: not complete
        };
        sampled.push(SectionVertex {
            offset,
            elevation: z,
        });
    }
    section.ground = sampled;
    section.complete = true;
    let formation = vertices(&section.formation);
    let whole = area_between(&section.ground, &formation);
    section.cut_area = whole.above; // ground above the formation
    section.fill_area = whole.below; // the formation above the ground
    let beyond = cpp::max(leftmost.abs(), rightmost.abs()) + 1.0;
    let left_split = area_between(
        &clipped(&section.ground, 0.0, beyond),
        &clipped(&formation, 0.0, beyond),
    );
    let right_split = area_between(
        &clipped(&section.ground, -beyond, 0.0),
        &clipped(&formation, -beyond, 0.0),
    );
    section.left_cut = left_split.above;
    section.left_fill = left_split.below;
    section.right_cut = right_split.above;
    section.right_fill = right_split.below;
    // Where the earthworks have thickness; a side with none has no cut (or no
    // fill) whatever sliver of area the daylight's last 1e-13 m left. This is
    // the one place that decides it: the end areas, the quantities and the
    // objects all follow.
    let formation_levels: Vec<f64> = section
        .ground
        .iter()
        .map(|sample| level_on(&formation, sample.offset))
        .collect();
    section.left_cut_spans = earthwork_spans(&section.ground, &formation_levels, Side::Left, true);
    section.left_fill_spans =
        earthwork_spans(&section.ground, &formation_levels, Side::Left, false);
    section.right_cut_spans =
        earthwork_spans(&section.ground, &formation_levels, Side::Right, true);
    section.right_fill_spans =
        earthwork_spans(&section.ground, &formation_levels, Side::Right, false);
    let mut trimmed = false;
    settle(&section.left_cut_spans, &mut section.left_cut, &mut trimmed);
    settle(
        &section.left_fill_spans,
        &mut section.left_fill,
        &mut trimmed,
    );
    settle(
        &section.right_cut_spans,
        &mut section.right_cut,
        &mut trimmed,
    );
    settle(
        &section.right_fill_spans,
        &mut section.right_fill,
        &mut trimmed,
    );
    if trimmed {
        section.cut_area = section.left_cut + section.right_cut;
        section.fill_area = section.left_fill + section.right_fill;
    }
    Ok(section)
}

fn part_of(kind: ComponentKind) -> &'static str {
    match kind {
        ComponentKind::Lane => "TRAFFICLANE",
        ComponentKind::Shoulder => "SHOULDER",
        ComponentKind::Footpath => "SIDEWALK",
        ComponentKind::Median => "CENTRALRESERVE",
        ComponentKind::Kerb
        | ComponentKind::Verge
        | ComponentKind::Drain
        | ComponentKind::Batter => "ROADSIDE",
    }
}

fn object_kind_of(kind: ComponentKind) -> &'static str {
    match kind {
        ComponentKind::Lane | ComponentKind::Shoulder => "pavement",
        ComponentKind::Footpath => "footpath",
        ComponentKind::Verge => "verge",
        ComponentKind::Median => "median",
        ComponentKind::Drain => "drain",
        ComponentKind::Kerb => "kerb",
        ComponentKind::Batter => "batter",
    }
}

// The outline in plan of a run of columns: the first column's line out, the
// last one's back.
fn outline_of(
    alignment: &SolvedAlignment,
    stations: &[f64],
    columns: &[Vec<SolidColumn>],
) -> Polyline2 {
    let mut outline = Polyline2::new(Vec::new(), true);
    for (&station, across) in stations.iter().zip(columns) {
        if let Some(p) = alignment.point_at_station_offset(station, across[0].offset) {
            outline.vertices.push(p);
        }
    }
    for (&station, across) in stations.iter().zip(columns).rev() {
        if let Some(p) = alignment.point_at_station_offset(station, across[across.len() - 1].offset)
        {
            outline.vertices.push(p);
        }
    }
    outline
}

fn plan_length(alignment: &SolvedAlignment, stations: &[f64], columns: &[Vec<SolidColumn>]) -> f64 {
    let mut length = CompensatedSum::default();
    let mut previous: Option<Point2> = None;
    for (&station, across) in stations.iter().zip(columns) {
        let Some(p) = alignment.point_at_station_offset(station, across[0].offset) else {
            continue;
        };
        if let Some(before) = previous {
            length.add(before.distance_to(p));
        }
        previous = Some(p);
    }
    length.value()
}

fn quantity(name: &str, value: f64, unit: &str) -> Quantity {
    Quantity {
        name: String::from(name),
        value,
        unit: String::from(unit),
    }
}

// ---- one section --------------------------------------------------------------------------

/// The section of `road_template` at `station`. `ground` may be None: the
/// batters then reach nothing and only the pavement is built.
/// `superelevation` is the corridor's table at the station
/// (superelevation_at; the C++ defaults it to none). InvalidArgument for a
/// template check_template refuses, or a station outside the alignment or the
/// profile.
pub fn road_section(
    road_template: &RoadTemplate,
    alignment: &SolvedAlignment,
    profile: &SolvedProfile,
    ground: Option<&TinSurface>,
    station: f64,
    superelevation: Option<Crossfalls>,
) -> Result<RoadSection> {
    check_template(road_template)?;
    build_section(
        road_template,
        alignment,
        profile,
        ground,
        station,
        superelevation,
    )
}

/// The template across a straight, level road at level 0 with no ground - its
/// pavement as designed: what ROADTEMPLATE SECTION prints and the road
/// template manager draws.
pub fn template_section(road_template: &RoadTemplate) -> Result<RoadSection> {
    let straight = HorizontalAlignment {
        pis: vec![
            AlignmentPi {
                point: Point2::new(0.0, 0.0),
                ..AlignmentPi::default()
            },
            AlignmentPi {
                point: Point2::new(100.0, 0.0),
                ..AlignmentPi::default()
            },
        ],
        ..HorizontalAlignment::default()
    };
    let level = VerticalAlignment {
        pvis: vec![
            ProfilePvi {
                station: 0.0,
                elevation: 0.0,
                ..ProfilePvi::default()
            },
            ProfilePvi {
                station: 100.0,
                elevation: 0.0,
                ..ProfilePvi::default()
            },
        ],
    };
    let (Ok(alignment), Ok(profile)) = (solve_alignment(&straight), solve_profile(&level)) else {
        return Err(Error::new(
            ErrorCode::Internal,
            "the straight a template is shown on did not solve",
        ));
    };
    road_section(road_template, &alignment, &profile, None, 50.0, None)
}

/// The stations a corridor is sectioned at from `start` to `end`: start + k *
/// interval (multiplied, so the hundredth is not off by a hundred roundings),
/// both ends, and each of `keys` inside, sorted, any two within
/// math::tolerance::GEOMETRIC made one.
pub fn section_stations(start: f64, end: f64, interval: f64, keys: &[f64]) -> Vec<f64> {
    let mut stations = Vec::new();
    if !(interval > 0.0) || !interval.is_finite() || !(end >= start) {
        return stations;
    }
    let mut k: usize = 0;
    loop {
        let station = start + k as f64 * interval;
        if !(station < end) {
            break;
        }
        stations.push(station);
        k += 1;
    }
    stations.push(end);
    for &key in keys {
        if key > start && key < end {
            stations.push(key);
        }
    }
    stations.sort_by(f64::total_cmp);
    stations.dedup_by(|b, a| (*a - *b).abs() <= tolerance::GEOMETRIC);
    stations
}

// The greatest |curvature| of the alignment anywhere from `from` to `to`
// (1/m): a tangent has none, an arc 1/radius, and a spiral's changes linearly,
// so its greatest is at one end of the part inside the range. The elements are
// asked rather than curvature_at_station, which at a joint answers for the
// element that begins there and so, at the end of a gap that stops where an
// arc stops, would say the tangent after it.
fn greatest_curvature(alignment: &SolvedAlignment, from: f64, to: f64) -> f64 {
    let mut greatest = 0.0;
    // From the element that holds `from` (they are in station order, end to
    // end), so a corridor of a hundred thousand sections along an alignment of
    // a thousand elements does not look at a thousand of them for each.
    let elements = alignment.elements();
    let at = elements
        .partition_point(|element| !(from < element.start_station))
        .saturating_sub(1);
    for element in &elements[at..] {
        if !(element.start_station < to) {
            break;
        }
        let begin = element.start_station;
        let finish = begin + element.length;
        if finish <= from || begin >= to {
            continue;
        }
        let low = cpp::max(from, begin) - begin;
        let high = cpp::min(to, finish) - begin;
        let here = match &element.shape {
            AlignmentShape::Arc(shape) => 1.0 / shape.radius.abs(),
            AlignmentShape::Spiral(shape) => cpp::max(
                shape.curvature_at(low).abs(),
                shape.curvature_at(high).abs(),
            ),
            AlignmentShape::Segment(_) => 0.0,
        };
        greatest = cpp::max(greatest, here);
    }
    greatest
}

/// The same as section_stations, with sections added inside the curves of
/// `alignment` so that no two neighbouring ones turn through more than
/// sqrt(6 x curve_tolerance) radians between them: a gap that turns through
/// more is cut into the fewest equal parts that do not (the C++ overload of
/// sectionStations that takes the alignment). The stations of the plain list
/// are all kept - the interval's multiples and every key - and a straight adds
/// none. Why that bound: the quads a solid is lofted from are chords, and over
/// a step of arc angle theta they hold sin(theta) / theta of an annulus
/// sector's area, 1 - theta^2 / 6 for small theta, so theta =
/// sqrt(6 x tolerance) keeps a course's area and volume within
/// `curve_tolerance` of the curve's, and a kerb's length (1 - theta^2 / 24)
/// within a quarter of it (docs/road_design.md, "Building a corridor"). A
/// spiral is held to the curvature at its sharper end of the gap, so it errs
/// on the safe side. `curve_tolerance` 0 (or not positive) adds none: the
/// plain list. InvalidArgument, saying what is asked, when the stations would
/// be more than 100 000 - a tolerance of 1e-12 on a long curve - before one is
/// made.
pub fn section_stations_along(
    start: f64,
    end: f64,
    interval: f64,
    keys: &[f64],
    alignment: &SolvedAlignment,
    curve_tolerance: f64,
) -> Result<Vec<f64>> {
    let plain = section_stations(start, end, interval, keys);
    if !(curve_tolerance > 0.0) || !curve_tolerance.is_finite() || plain.len() < 2 {
        return Ok(plain);
    }
    // The turn one step may make: 1 - sin(t) / t is t^2 / 6 for small t.
    let turn = (6.0 * curve_tolerance).sqrt();
    let mut stations = Vec::with_capacity(plain.len());
    for (i, pair) in plain.windows(2).enumerate() {
        let (a, b) = (pair[0], pair[1]);
        stations.push(a);
        // The fewest equal parts that each turn through no more than `turn`,
        // counted as a real first: a tolerance of 1e-300 asks for more parts
        // than a usize holds. RELATIVE keeps a gap that turns through exactly
        // `turn` from being cut for the rounding of the quotient.
        let parts =
            (greatest_curvature(alignment, a, b) * (b - a) / turn - tolerance::RELATIVE).ceil();
        if !(parts > 1.0) {
            continue;
        }
        // What the list would hold: those made, the parts' new ones, the rest.
        let total = (stations.len() + plain.len() - i - 1) as f64 + (parts - 1.0);
        if total > MAXIMUM_SECTIONS as f64 {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                format!(
                    "a curve tolerance of {} needs more than {MAXIMUM_SECTIONS} sections along \
                     this alignment: not built",
                    number(curve_tolerance)
                ),
                format!("stations {} to {}", number(start), number(end)),
            ));
        }
        let count = parts as usize;
        for j in 1..count {
            stations.push(a + (b - a) * j as f64 / count as f64);
        }
    }
    stations.push(plain[plain.len() - 1]);
    Ok(stations)
}

// ---- a whole corridor -------------------------------------------------------------------

/// The stretch of a corridor built with one template: a region of its own, or
/// the corridor's template between and around the regions. Numbered from 1 in
/// station order; the number is in its objects' ids.
#[derive(Clone, Debug, PartialEq)]
pub struct CorridorRun {
    pub region: usize,
    pub road_template: RoadTemplate,
    pub start: f64,
    pub end: f64,
    /// Ascending station, both ends included.
    pub sections: Vec<RoadSection>,
    /// End-area cut and fill each side, over the intervals between two
    /// complete sections; the sections that are not complete.
    pub left_cut: f64,
    pub left_fill: f64,
    pub right_cut: f64,
    pub right_fill: f64,
    pub incomplete_sections: usize,
}

impl Default for CorridorRun {
    fn default() -> Self {
        Self {
            region: 1,
            road_template: RoadTemplate::default(),
            start: 0.0,
            end: 0.0,
            sections: Vec::new(),
            left_cut: 0.0,
            left_fill: 0.0,
            right_cut: 0.0,
            right_fill: 0.0,
            incomplete_sections: 0,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct BuiltCorridor {
    pub name: String,
    /// The target surface it names, as the drawing spells it (the corridor
    /// may have been typed in another case); empty for none.
    pub surface: String,
    pub alignment: Option<SolvedAlignment>,
    /// The stations it was built over. Meaningful only when has_range: a
    /// corridor that could not be built (no alignment, a range off it) has
    /// none, and station 0 is a real station, not a placeholder.
    pub start: f64,
    pub end: f64,
    pub has_range: bool,
    pub runs: Vec<CorridorRun>,
    /// What could not be built, a sentence each: an alignment, a profile, a
    /// template or a surface that is not there, a station range outside the
    /// alignment, sections that do not reach the ground.
    pub problems: Vec<String>,
    /// Cubic metres, both sides, every run.
    pub cut: f64,
    pub fill: f64,
    /// Whether cut and fill were measured at all: there is a target surface
    /// and at least one run was built. Without, they are zero only because
    /// nothing was measured, which is not the zero of a road with no cut.
    pub has_earthworks: bool,
    pub sections: usize,
    pub incomplete_sections: usize,
}

/// The stations a corridor is built over: its own range - an end it does not
/// give is the alignment's and the profile's overlap - held to that overlap.
/// `ok` is false when there is nothing to build (no overlap, or a range off
/// it), and `problems` says in a sentence each what was held to the overlap or
/// why nothing is built.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CorridorRange {
    pub start: f64,
    pub end: f64,
    pub ok: bool,
    pub problems: Vec<String>,
}

pub fn corridor_range(
    corridor: &Corridor,
    alignment: &SolvedAlignment,
    profile: &SolvedProfile,
) -> CorridorRange {
    let mut range = CorridorRange::default();
    if let Err(error) = check_corridor(corridor) {
        range.problems.push(error.message);
        return range;
    }
    let low = cpp::max(alignment.start_station(), profile.start_station());
    let high = cpp::min(alignment.end_station(), profile.end_station());
    if !(high - low > tolerance::COORDINATE) {
        range.problems.push(format!(
            "the alignment {} and its design profile do not overlap in station",
            corridor.alignment
        ));
        return range;
    }
    let mut start = corridor.start.unwrap_or(low);
    let mut end = corridor.end.unwrap_or(high);
    if start < low {
        range.problems.push(format!(
            "the corridor starts at {}, before its alignment and profile begin: built from {}",
            number(start),
            number(low)
        ));
        start = low;
    }
    if end > high {
        range.problems.push(format!(
            "the corridor ends at {}, after its alignment and profile end: built to {}",
            number(end),
            number(high)
        ));
        end = high;
    }
    if !(end - start > tolerance::COORDINATE) {
        range.problems.push(format!(
            "the corridor's stations {} to {} are not on its alignment and profile",
            number(start),
            number(end)
        ));
        return range;
    }
    range.start = start;
    range.end = end;
    range.ok = true;
    range
}

/// Builds what it can. A corridor whose alignment or profile is missing has
/// no runs; a template that is missing or refused leaves its stretch out; no
/// target surface builds the pavement alone. Every such thing is a problem.
pub fn build_corridor(
    corridor: &Corridor,
    templates: &RoadTemplateSet,
    alignment: &SolvedAlignment,
    profile: &SolvedProfile,
    ground: Option<&TinSurface>,
) -> BuiltCorridor {
    let mut built = BuiltCorridor {
        name: corridor.name.clone(),
        surface: corridor.surface.clone(),
        alignment: Some(alignment.clone()),
        ..BuiltCorridor::default()
    };
    let range = corridor_range(corridor, alignment, profile);
    built.problems.extend(range.problems.iter().cloned());
    if !range.ok {
        return built;
    }
    let start = range.start;
    let end = range.end;
    if (end - start) / corridor.interval > MAXIMUM_SECTIONS as f64 {
        built.problems.push(format!(
            "an interval of {} over {} m is more than {MAXIMUM_SECTIONS} sections: not built",
            number(corridor.interval),
            number(end - start)
        ));
        return built;
    }

    // Every key station: where the geometry, the grade, the superelevation or
    // the template changes character, which a schedule must not step over.
    let mut keys = alignment.key_stations();
    keys.extend(profile.key_stations());
    for point in &corridor.superelevation {
        keys.push(point.station);
    }
    for region in &corridor.regions {
        keys.push(region.start);
        keys.push(region.end);
    }
    // And the stations a curve needs for its quantities to be within the
    // corridor's tolerance. Refused, like an interval that makes too many,
    // before any section is made, and with no range: nothing was built.
    let stations = match section_stations_along(
        start,
        end,
        corridor.interval,
        &keys,
        alignment,
        corridor.curve_tolerance,
    ) {
        Ok(stations) => stations,
        Err(error) => {
            built.problems.push(error.message);
            return built;
        }
    };
    built.start = start;
    built.end = end;
    built.has_range = true;

    // The runs: the regions in station order, and the corridor's template
    // before, between and after them.
    struct Stretch<'a> {
        from: f64,
        to: f64,
        template_name: &'a str,
    }
    let mut stretches: Vec<Stretch<'_>> = Vec::new();
    let mut cursor = start;
    for region in &corridor.regions {
        let from = cpp::max(region.start, start);
        let to = cpp::min(region.end, end);
        if !(to - from > tolerance::COORDINATE) {
            built.problems.push(format!(
                "the region {} to {} is outside the corridor's stations",
                number(region.start),
                number(region.end)
            ));
            continue;
        }
        if from - cursor > tolerance::COORDINATE {
            stretches.push(Stretch {
                from: cursor,
                to: from,
                template_name: &corridor.template_name,
            });
        }
        stretches.push(Stretch {
            from,
            to,
            template_name: &region.template_name,
        });
        cursor = to;
    }
    if end - cursor > tolerance::COORDINATE {
        stretches.push(Stretch {
            from: cursor,
            to: end,
            template_name: &corridor.template_name,
        });
    }

    let mut batters = false;
    let mut cut = CompensatedSum::default();
    let mut fill = CompensatedSum::default();
    for (n, stretch) in stretches.iter().enumerate() {
        let Some(road_template) = templates.find(stretch.template_name) else {
            built.problems.push(format!(
                "the road template {} (stations {} to {}) is not in the drawing: that stretch is \
                 not built",
                stretch.template_name,
                number(stretch.from),
                number(stretch.to)
            ));
            continue;
        };
        if let Err(error) = check_template(road_template) {
            built.problems.push(error.message);
            continue;
        }
        if road_template.components.is_empty() {
            built.problems.push(format!(
                "the road template {} has no components: stations {} to {} have nothing to build",
                road_template.name,
                number(stretch.from),
                number(stretch.to)
            ));
        }
        batters = batters
            || road_template
                .components
                .iter()
                .any(|component| component.kind == ComponentKind::Batter);
        let mut run = CorridorRun {
            region: n + 1,
            road_template: road_template.clone(),
            start: stretch.from,
            end: stretch.to,
            ..CorridorRun::default()
        };
        for &station in &stations {
            if station < stretch.from - tolerance::GEOMETRIC
                || station > stretch.to + tolerance::GEOMETRIC
            {
                continue;
            }
            let at = cpp::clamp(station, stretch.from, stretch.to);
            let mut section = match build_section(
                road_template,
                alignment,
                profile,
                ground,
                at,
                superelevation_at(corridor, at),
            ) {
                Ok(section) => section,
                Err(error) => {
                    built.problems.push(error.describe());
                    continue;
                }
            };
            if !section.complete {
                run.incomplete_sections += 1;
            }
            section.run = run.region;
            run.sections.push(section);
        }
        // End areas between two complete sections; an interval touching an
        // incomplete one adds nothing, and the count says so.
        let mut left_cut = CompensatedSum::default();
        let mut left_fill = CompensatedSum::default();
        let mut right_cut = CompensatedSum::default();
        let mut right_fill = CompensatedSum::default();
        for pair in run.sections.windows(2) {
            let (a, b) = (&pair[0], &pair[1]);
            if !a.complete || !b.complete {
                continue;
            }
            let length = b.station - a.station;
            left_cut.add(0.5 * (a.left_cut + b.left_cut) * length);
            left_fill.add(0.5 * (a.left_fill + b.left_fill) * length);
            right_cut.add(0.5 * (a.right_cut + b.right_cut) * length);
            right_fill.add(0.5 * (a.right_fill + b.right_fill) * length);
        }
        run.left_cut = left_cut.value();
        run.left_fill = left_fill.value();
        run.right_cut = right_cut.value();
        run.right_fill = right_fill.value();
        cut.add(run.left_cut);
        cut.add(run.right_cut);
        fill.add(run.left_fill);
        fill.add(run.right_fill);
        built.sections += run.sections.len();
        built.incomplete_sections += run.incomplete_sections;
        if ground.is_some() && run.incomplete_sections > 0 {
            built.problems.push(format!(
                "{} of {} sections of stations {} to {} do not reach the ground: their earthworks \
                 are left out",
                run.incomplete_sections,
                run.sections.len(),
                number(run.start),
                number(run.end)
            ));
        }
        built.runs.push(run);
    }
    if ground.is_none() && batters {
        built.problems.push(String::from(
            "no target surface: the batters and the earthworks are not built",
        ));
    }
    built.cut = cut.value();
    built.fill = fill.value();
    built.has_earthworks = ground.is_some() && !built.runs.is_empty();
    built
}

/// What a corridor names, found in the drawing: its alignment and design
/// profile solved, its target surface (None when it names none, or one the
/// drawing lacks), and a sentence for each that is missing or does not solve.
/// No alignment or no profile: both absent.
#[derive(Clone, Debug, Default)]
pub struct CorridorInputs {
    pub alignment: Option<SolvedAlignment>,
    pub profile: Option<SolvedProfile>,
    /// The drawing's own surface, shared (the C++ holds a pointer to it).
    pub ground: Option<Arc<TinSurface>>,
    /// The surface's name as the drawing spells it; empty with no ground.
    pub surface_name: String,
    pub problems: Vec<String>,
}

pub fn corridor_inputs(document: &Document, corridor: &Corridor) -> CorridorInputs {
    let mut inputs = CorridorInputs::default();
    let Some(alignment) = document.model().alignments.find(&corridor.alignment) else {
        inputs.problems.push(format!(
            "the alignment {} is not in the drawing",
            corridor.alignment
        ));
        return inputs;
    };
    let Some(vertical) = &alignment.vertical else {
        inputs.problems.push(format!(
            "the alignment {} has no design profile (ALIGN DESIGN)",
            corridor.alignment
        ));
        return inputs;
    };
    let solved = match solve_alignment(&alignment.horizontal) {
        Ok(solved) => solved,
        Err(error) => {
            inputs.problems.push(error.describe());
            return inputs;
        }
    };
    let profile = match solve_profile(vertical) {
        Ok(profile) => profile,
        Err(error) => {
            inputs.problems.push(error.describe());
            return inputs;
        }
    };
    inputs.alignment = Some(solved);
    inputs.profile = Some(profile);
    if !corridor.surface.is_empty() {
        match document.surfaces().find(&corridor.surface) {
            Some(named) if named.surface.is_some() => {
                inputs.ground = named.surface.clone();
                inputs.surface_name = named.name.clone();
            }
            _ => inputs.problems.push(format!(
                "the surface {} is not in the drawing",
                corridor.surface
            )),
        }
    }
    inputs
}

/// The same as build_corridor from the drawing (the C++ overload
/// `buildCorridor(const Document&, const Corridor&)`): its inputs
/// (corridor_inputs) and the drawing's road templates
/// (Document::road_templates).
pub fn build_corridor_in(document: &Document, corridor: &Corridor) -> BuiltCorridor {
    let inputs = corridor_inputs(document, corridor);
    let (Some(alignment), Some(profile)) = (&inputs.alignment, &inputs.profile) else {
        return BuiltCorridor {
            name: corridor.name.clone(),
            problems: inputs.problems,
            ..BuiltCorridor::default()
        };
    };
    let templates = document.road_templates();
    let mut built = build_corridor(
        corridor,
        &templates,
        alignment,
        profile,
        inputs.ground.as_deref(),
    );
    // The inputs' problems first.
    let mut problems = inputs.problems.clone();
    problems.append(&mut built.problems);
    built.problems = problems;
    // The surface as the drawing spells it: the lookup ignores case, and the
    // IFC export finds the cut's ground by this name, exactly.
    if !inputs.surface_name.is_empty() {
        built.surface = inputs.surface_name.clone();
    }
    built
}

// ---- solids -------------------------------------------------------------------------------

// A face that names one vertex twice is a welded edge's: it has no area and is
// not made, which leaves each edge it touched with the two faces it had.
fn push_triangle(faces: &mut Vec<[u32; 3]>, a: u32, b: u32, c: u32) {
    if a != b && b != c && a != c {
        faces.push([a, b, c]);
    }
}

fn push_quad(faces: &mut Vec<[u32; 3]>, a: u32, b: u32, c: u32, d: u32) {
    push_triangle(faces, a, b, c);
    push_triangle(faces, a, c, d);
}

/// A closed solid from a solid's columns at a run of stations: a top and a
/// bottom face over the columns, the inner and outer faces down the first and
/// last column, and a cap at each end - each edge shared by exactly two
/// triangles, the faces turned outward. `columns[k]` are station `k`'s, all
/// with the same count (two at least); `stations` has two at least.
/// InvalidArgument otherwise.
///
/// A solid may TAPER. Where a column's top is no more than `weld_below` above
/// its bottom, in a station's first or last column, or in any column of the
/// first or last station, the two are ONE vertex (at the top's level), and the
/// faces that would have run between them - which have no area - are not
/// made: the solid ends in a line, a wedge welded to it, and is closed as
/// before. A cell whose four corners are all welded is two faces on one
/// surface and is left out. An interior column is never welded, so a solid
/// cannot pinch to a line with a body on each side. The C++ default, 0, welds
/// only a column of no thickness at all, which no course or kerb has
/// (docs/road_design.md, "A closed solid").
///
/// A closed, outward-turned mesh's volume is geometry's mesh_volume.
#[allow(
    clippy::needless_range_loop,
    reason = "the indices name a station and a column of the solid's grid, as the C++ loops do"
)]
pub fn loft_solid(
    alignment: &SolvedAlignment,
    stations: &[f64],
    columns: &[Vec<SolidColumn>],
    weld_below: f64,
) -> Result<TriangleMesh> {
    let count = stations.len();
    if count < 2 || columns.len() != count {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "a solid needs its columns at two stations at least",
        ));
    }
    let across = columns[0].len();
    if across < 2 || columns.iter().any(|station| station.len() != across) {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "a solid needs the same columns, two at least, at every station",
        ));
    }
    let last = across - 1;
    // Where the solid has no thickness its top and bottom are ONE vertex. The
    // columns that may: a station's first and last (a solid ends there in a
    // line, as an earthworks stretch does where the ground meets the
    // formation) and every column of the first and last station (it tapers to
    // a line there). An interior column is never welded, so a solid cannot
    // pinch to a line with a body each side, an edge four faces use.
    let welded = |k: usize, i: usize| -> bool {
        let column = &columns[k][i];
        column.top - column.bottom <= weld_below
            && (i == 0 || i == last || k == 0 || k + 1 == count)
    };
    let mut mesh = TriangleMesh::default();
    mesh.vertices.reserve(count * across * 2);
    let mut top_at = vec![0_u32; count * across];
    let mut bottom_at = vec![0_u32; count * across];
    for k in 0..count {
        for i in 0..across {
            let column = &columns[k][i];
            let Some(plan) = alignment.point_at_station_offset(stations[k], column.offset) else {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    "the station is outside the alignment",
                    number(stations[k]),
                ));
            };
            top_at[k * across + i] = mesh.vertices.len() as u32;
            mesh.vertices.push(Point3::new(plan.x, plan.y, column.top));
            if welded(k, i) {
                bottom_at[k * across + i] = top_at[k * across + i];
            } else {
                bottom_at[k * across + i] = mesh.vertices.len() as u32;
                mesh.vertices
                    .push(Point3::new(plan.x, plan.y, column.bottom));
            }
        }
    }
    let vertex = |k: usize, i: usize, bottom: bool| -> u32 {
        if bottom {
            bottom_at[k * across + i]
        } else {
            top_at[k * across + i]
        }
    };
    let faces = &mut mesh.faces;
    // Each quad is walked so that every edge it shares is walked the other
    // way by its neighbour (docs/road_design.md, "A closed solid").
    for k in 0..count - 1 {
        for i in 0..across - 1 {
            // A triangle of the cell with nothing between its top and bottom
            // at all three corners is two faces on one surface, which nothing
            // is inside: left out, top and bottom both, it leaves the faces
            // beside it closed along its edges. The cell is split along its
            // diagonal (k, i) to (k + 1, i + 1): the first triangle has the
            // corner (k, i + 1), the second (k + 1, i), and each one's bottom
            // is its top turned over. (Left in, the pair makes the diagonal an
            // edge four faces use - found by `closed_on_its_indices` on the
            // pieces of a fill that tapers away along the road.)
            let first = !(welded(k, i) && welded(k, i + 1) && welded(k + 1, i + 1));
            let second = !(welded(k, i) && welded(k + 1, i + 1) && welded(k + 1, i));
            if first {
                push_triangle(
                    faces,
                    vertex(k, i, false),
                    vertex(k, i + 1, false),
                    vertex(k + 1, i + 1, false),
                );
                push_triangle(
                    faces,
                    vertex(k, i, true),
                    vertex(k + 1, i + 1, true),
                    vertex(k, i + 1, true),
                );
            }
            if second {
                push_triangle(
                    faces,
                    vertex(k, i, false),
                    vertex(k + 1, i + 1, false),
                    vertex(k + 1, i, false),
                );
                push_triangle(
                    faces,
                    vertex(k, i, true),
                    vertex(k + 1, i, true),
                    vertex(k + 1, i + 1, true),
                );
            }
        }
        push_quad(
            faces,
            vertex(k, 0, false),
            vertex(k + 1, 0, false),
            vertex(k + 1, 0, true),
            vertex(k, 0, true),
        );
        push_quad(
            faces,
            vertex(k + 1, last, false),
            vertex(k, last, false),
            vertex(k, last, true),
            vertex(k + 1, last, true),
        );
    }
    for i in 0..across - 1 {
        push_quad(
            faces,
            vertex(0, i + 1, false),
            vertex(0, i, false),
            vertex(0, i, true),
            vertex(0, i + 1, true),
        );
        let k = count - 1;
        push_quad(
            faces,
            vertex(k, i, false),
            vertex(k, i + 1, false),
            vertex(k, i + 1, true),
            vertex(k, i, true),
        );
    }
    // A welded vertex whose every face was left out (the end of a line of no
    // thickness that a neighbouring triangle took) is in no face: not kept.
    {
        const UNUSED: u32 = u32::MAX;
        let mut placed = vec![UNUSED; mesh.vertices.len()];
        for face in &mesh.faces {
            for &index in face {
                placed[index as usize] = 0;
            }
        }
        let mut kept = Vec::with_capacity(mesh.vertices.len());
        for (v, point) in mesh.vertices.iter().enumerate() {
            if placed[v] != UNUSED {
                placed[v] = kept.len() as u32;
                kept.push(*point);
            }
        }
        if kept.len() != mesh.vertices.len() {
            for face in &mut mesh.faces {
                for index in face.iter_mut() {
                    *index = placed[*index as usize];
                }
            }
            mesh.vertices = kept;
        }
    }
    // Which way that turns depends on the side: offsets grow to the left, so
    // the right side's solids are mirror images. Turned outward by the sign of
    // the volume it encloses.
    if mesh_volume(&mesh) < 0.0 {
        for face in &mut mesh.faces {
            face.swap(1, 2);
        }
    }
    Ok(mesh)
}

fn spans_of(section: &RoadSection, side: Side, cutting: bool) -> &[OutSpan] {
    match (side, cutting) {
        (Side::Left, true) => &section.left_cut_spans,
        (Side::Left, false) => &section.left_fill_spans,
        (Side::Right, true) => &section.right_cut_spans,
        (Side::Right, false) => &section.right_fill_spans,
    }
}

fn area_of(section: &RoadSection, side: Side, cutting: bool) -> f64 {
    match (side, cutting) {
        (Side::Left, true) => section.left_cut,
        (Side::Left, false) => section.left_fill,
        (Side::Right, true) => section.right_cut,
        (Side::Right, false) => section.right_fill,
    }
}

// One shell of the solid: stretch `stretch` (counted from the centreline) of
// the sections `begin` to `finish` of `run`. A section that has no such
// stretch - the one beyond each end of the shell - is the line along the
// formation at the stretch of its neighbour in the shell, with no thickness
// there. The C++ answers false and the reason through an argument; here the
// reason is the error.
#[allow(
    clippy::too_many_arguments,
    reason = "the C++ earthworkShell's arguments, with the alignment it reads from the built corridor"
)]
fn earthwork_shell(
    alignment: &SolvedAlignment,
    run: &CorridorRun,
    side: Side,
    cutting: bool,
    stretch: usize,
    begin: usize,
    finish: usize,
    solid: &mut TriangleMesh,
) -> std::result::Result<(), String> {
    let sign = if side == Side::Left { 1.0 } else { -1.0 };
    let mut stations = Vec::new();
    let mut columns: Vec<Vec<SolidColumn>> = Vec::new();
    for m in begin..=finish {
        let section = &run.sections[m];
        let has = spans_of(section, side, cutting).len() > stretch;
        let like = if has {
            m
        } else if m == begin {
            m + 1
        } else {
            m - 1
        };
        let span = spans_of(&run.sections[like], side, cutting)[stretch];
        let formation = vertices(&section.formation);
        let mut across = Vec::with_capacity(EARTHWORKS_COLUMNS);
        for c in 0..EARTHWORKS_COLUMNS {
            let out =
                span.from + (span.to - span.from) * c as f64 / (EARTHWORKS_COLUMNS - 1) as f64;
            let offset = sign * out;
            let f = level_on(&formation, offset);
            if !has {
                across.push(SolidColumn {
                    offset,
                    top: f,
                    bottom: f,
                });
                continue;
            }
            let g = level_on(&section.ground, offset);
            across.push(if cutting {
                SolidColumn {
                    offset,
                    top: cpp::max(g, f),
                    bottom: f,
                }
            } else {
                SolidColumn {
                    offset,
                    top: f,
                    bottom: cpp::min(g, f),
                }
            });
        }
        stations.push(section.station);
        columns.push(across);
    }
    // A thickness under the model's 0.1 mm is the earthworks' own "none"
    // (earthwork_spans): welded, where a solid may end in a line.
    let mesh = loft_solid(alignment, &stations, &columns, tolerance::COORDINATE)
        .map_err(|error| error.message)?;
    let base = solid.vertices.len() as u32;
    solid.vertices.extend_from_slice(&mesh.vertices);
    for face in &mesh.faces {
        solid
            .faces
            .push([face[0] + base, face[1] + base, face[2] + base]);
    }
    Ok(())
}

// The cut (or fill) of one side of one run, as a closed solid for each piece:
// a stretch of stations over which the side has earthworks at all
// (RoadSection::left_cut_spans ...), inside a stretch of complete sections.
// Each piece runs over the sections that have earthworks and one beyond each
// end where there is one (a section with none), because the end-area volume
// of the interval between the two tapers to nothing there. Its columns run
// from where its earthworks start across the section to where they end, so
// the solid, its plan outline and its GrossArea are the earthworks and not the
// whole half-section between the centreline and the daylight.
//
// The solid is a closed SHELL for each stretch across (earthwork_shell): a
// section whose earthworks are in two stretches - cut, fill, cut across one
// half-section - makes two, and no column spans the gap between them with no
// thickness, where the top and the bottom of one solid would lie on one
// surface. And a shell ends where its stretch does in a line, not a face of no
// area: the section beyond a piece's end is a line along the formation, and
// the edge of a stretch, where the ground meets the formation, is one too
// (loft_solid, which welds them).
fn add_earthworks(
    model: &mut BimModel,
    built: &BuiltCorridor,
    alignment: &SolvedAlignment,
    run: &CorridorRun,
    side: Side,
    cutting: bool,
) {
    let source = format!("corridor {}", built.name);
    let side_word = side.name();
    let region = format!("{}", run.region);
    let what = if cutting { "cut" } else { "fill" };
    let sign = if side == Side::Left { 1.0 } else { -1.0 };
    let active = |index: usize| !spans_of(&run.sections[index], side, cutting).is_empty();
    let mut emitted: usize = 0;
    let mut k = 0;
    while k < run.sections.len() {
        if !run.sections[k].complete {
            k += 1;
            continue;
        }
        let mut last = k;
        while last + 1 < run.sections.len() && run.sections[last + 1].complete {
            last += 1;
        }
        let first = k;
        k = last + 1;
        if last == first {
            continue;
        }
        // The runs of sections that have earthworks, inside [first, last].
        let mut next = first;
        while next <= last {
            let j = next;
            next = j + 1;
            if !active(j) {
                continue;
            }
            let mut last_active = j;
            while last_active < last && active(last_active + 1) {
                last_active += 1;
            }
            let first_active = j;
            next = last_active + 1;
            let from = if first_active > first {
                first_active - 1
            } else {
                first_active
            };
            let to = if last_active < last {
                last_active + 1
            } else {
                last_active
            };
            if from == to {
                continue;
            }
            let mut end_area = CompensatedSum::default();
            for m in from..to {
                let a = &run.sections[m];
                let b = &run.sections[m + 1];
                end_area.add(
                    0.5 * (area_of(a, side, cutting) + area_of(b, side, cutting))
                        * (b.station - a.station),
                );
            }
            if !(end_area.value() > 0.0) {
                continue;
            }
            // The plan outline: where the earthworks start across each section
            // to where they end, the hull of its stretches. A section with none
            // takes the stretch of its neighbour that has some, and in plan is a
            // point at the middle of it, which makes the outline taper as the
            // end-area volume does.
            let mut stations: Vec<f64> = Vec::new();
            let mut inner: Vec<Point2> = Vec::new();
            let mut outer: Vec<Point2> = Vec::new();
            let mut stretches: usize = 0;
            for m in from..=to {
                let section = &run.sections[m];
                let has = active(m);
                let like = if has {
                    m
                } else if m == from {
                    m + 1
                } else {
                    m - 1
                };
                let spans = spans_of(&run.sections[like], side, cutting);
                let out_from = spans[0].from;
                let out_to = spans[spans.len() - 1].to;
                stretches = stretches.max(spans_of(section, side, cutting).len());
                stations.push(section.station);
                let lo = if has {
                    out_from
                } else {
                    0.5 * (out_from + out_to)
                };
                let hi = if has { out_to } else { lo };
                if let Some(p) = alignment.point_at_station_offset(section.station, sign * lo) {
                    inner.push(p);
                }
                if let Some(p) = alignment.point_at_station_offset(section.station, sign * hi) {
                    outer.push(p);
                }
            }
            // The solid: a shell for each stretch across, over the sections
            // that have it and one beyond each end that is in the piece.
            let mut solid = TriangleMesh::default();
            let mut why = String::new();
            let mut made = true;
            'stretches: for stretch in 0..stretches {
                let has = |m: usize| spans_of(&run.sections[m], side, cutting).len() > stretch;
                let mut next_m = from;
                while next_m <= to {
                    let m = next_m;
                    next_m = m + 1;
                    if !has(m) {
                        continue;
                    }
                    let mut end = m;
                    while end < to && has(end + 1) {
                        end += 1;
                    }
                    let begin = if m > from { m - 1 } else { m };
                    let finish = if end < to { end + 1 } else { end };
                    next_m = end + 1;
                    if begin == finish {
                        continue;
                    }
                    if let Err(message) = earthwork_shell(
                        alignment, run, side, cutting, stretch, begin, finish, &mut solid,
                    ) {
                        why = message;
                        made = false;
                        break 'stretches;
                    }
                }
            }
            if !made {
                model.problems.push(BimProblem {
                    source: source.clone(),
                    message: format!("earthworks {side_word}: {why}"),
                });
                continue;
            }
            let mut outline = Polyline2::new(inner, true);
            for point in outer.iter().rev() {
                // A tapered end is one point, not two.
                if outline.vertices.last() != Some(point) {
                    outline.vertices.push(*point);
                }
            }
            if outline.vertices.len() > 2 && outline.vertices.last() == outline.vertices.first() {
                outline.vertices.pop(); // the outline is closed: no point twice
            }
            emitted += 1;
            let piece = if emitted > 1 {
                format!("-{emitted}")
            } else {
                String::new()
            };
            let mut object = BimObject {
                id: format!(
                    "corridor/{}/{side_word}/earthworks/{what}/{region}{piece}",
                    built.name
                ),
                name: format!("{} {side_word} earthworks {what}", built.name),
                kind: String::from("earthworks"),
                facility: built.name.clone(),
                part: String::from("ROADSEGMENT"),
                ifc_class: String::from(if cutting {
                    "IfcEarthworksCut"
                } else {
                    "IfcEarthworksFill"
                }),
                predefined_type: String::from(if cutting { "CUT" } else { "EMBANKMENT" }),
                material: String::from(if cutting {
                    "excavated material"
                } else {
                    "fill"
                }),
                // As a cut and fill map colours them (the "Cut Fill" surface
                // style): blue cut, red fill.
                colour: if cutting {
                    Color::new(74, 127, 193, 255)
                } else {
                    Color::new(193, 80, 74, 255)
                },
                layer: String::from(if cutting {
                    "road/earthworks/cut"
                } else {
                    "road/earthworks/fill"
                }),
                ..BimObject::default()
            };
            object.plan.push(outline);
            object.properties = vec![
                (String::from("template"), run.road_template.name.clone()),
                (String::from("region"), region.clone()),
                (String::from("from"), number(stations[0])),
                (String::from("to"), number(stations[stations.len() - 1])),
                (String::from("side"), String::from(side_word)),
                (String::from("method"), String::from("average end area")),
            ];
            // The surface a cut is taken from, which the IFC export reads by
            // this name.
            if cutting {
                object
                    .properties
                    .push((String::from("Target Surface"), built.surface.clone()));
            }
            object.quantities = vec![
                quantity("Length", stations[stations.len() - 1] - stations[0], "m"),
                quantity("GrossArea", object.plan[0].area(), "m2"),
                quantity("GrossVolume", end_area.value(), "m3"),
            ];
            object.mesh = solid;
            model.objects.push(object);
        }
    }
}

fn fail(model: &mut BimModel, source: &str, what: String) {
    model.problems.push(BimProblem {
        source: String::from(source),
        message: what,
    });
}

/// The road model's objects of a built corridor (bim::bim_object): a closed
/// solid per course of each component per side per run, per kerb, and the
/// earthworks cut and fill of each side of each run - a piece for each
/// stretch of stations where that side has any, inside a stretch of complete
/// sections - with their quantities, and its problems.
///
/// Object ids (stable, docs/road_design.md, "Object ids"):
///
/// ```text
/// corridor/<corridor>/<side>/<component>/<course>/<run>
/// corridor/<corridor>/<side>/<component>/<run>                a kerb
/// corridor/<corridor>/<side>/earthworks/cut|fill/<run>[-<piece>]   (<piece> from 2)
/// ```
pub fn corridor_solids(built: &BuiltCorridor) -> BimModel {
    let mut model = BimModel::default();
    let source = format!("corridor {}", built.name);
    for problem in &built.problems {
        model.problems.push(BimProblem {
            source: source.clone(),
            message: problem.clone(),
        });
    }
    let Some(alignment) = &built.alignment else {
        return model;
    };
    for run in &built.runs {
        let region = format!("{}", run.region);
        if run.sections.len() < 2 {
            fail(
                &mut model,
                &source,
                format!(
                    "stations {} to {} have fewer than two sections: no solids are made for them",
                    number(run.start),
                    number(run.end)
                ),
            );
            continue;
        }
        let stations: Vec<f64> = run.sections.iter().map(|section| section.station).collect();
        let range: Vec<(String, String)> = vec![
            (String::from("template"), run.road_template.name.clone()),
            (String::from("region"), region.clone()),
            (String::from("from"), number(run.start)),
            (String::from("to"), number(run.end)),
        ];

        // ---- courses and kerbs ----------------------------------------------------------
        for (s, first) in run.sections[0].solids.iter().enumerate() {
            let mut columns: Vec<Vec<SolidColumn>> = Vec::new();
            for section in &run.sections {
                if s >= section.solids.len() || section.solids[s].key != first.key {
                    break;
                }
                columns.push(section.solids[s].columns.clone());
            }
            if columns.len() != stations.len() {
                fail(
                    &mut model,
                    &source,
                    format!(
                        "the solid {} changes between sections: it is not made",
                        first.key
                    ),
                );
                continue;
            }
            let mesh = match loft_solid(alignment, &stations, &columns, 0.0) {
                Ok(mesh) => mesh,
                Err(error) => {
                    fail(
                        &mut model,
                        &source,
                        format!("{}: {}", first.key, error.message),
                    );
                    continue;
                }
            };
            let component = &run.road_template.components[first.component];
            let side_word = component.side.name();
            let mut object = BimObject {
                facility: built.name.clone(),
                part: String::from(part_of(component.kind)),
                layer: component.layer.clone(),
                ..BimObject::default()
            };
            object.plan.push(outline_of(alignment, &stations, &columns));
            object.properties = range.clone();
            object
                .properties
                .push((String::from("side"), String::from(side_word)));
            object
                .properties
                .push((String::from("component"), component.name.clone()));
            let volume = mesh_volume(&mesh);
            if let Some(course_index) = first.course {
                let course = &component.courses[course_index];
                object.id = format!(
                    "corridor/{}/{side_word}/{}/{}/{region}",
                    built.name, component.name, course.name
                );
                object.name = format!(
                    "{} {side_word} {} {}",
                    built.name, component.name, course.name
                );
                object.kind = String::from(object_kind_of(component.kind));
                // A footpath's top course is the path itself, a slab (IfcSlab
                // SIDEWALK); what is under it, and every other course, an
                // IfcCourse of the type its template names.
                if component.kind == ComponentKind::Footpath && course_index == 0 {
                    object.ifc_class = String::from("IfcSlab");
                    object.predefined_type = String::from("SIDEWALK");
                } else {
                    object.ifc_class = String::from("IfcCourse");
                    object.predefined_type = course.ifc_type.clone();
                }
                object.material = course.material.clone();
                object.colour = course.colour;
                object
                    .properties
                    .push((String::from("course"), course.name.clone()));
                let front = &columns[0];
                let width = (front[front.len() - 1].offset - front[0].offset).abs();
                object.quantities = vec![
                    quantity("Length", run.end - run.start, "m"),
                    quantity("Width", width, "m"),
                    quantity("Depth", course.thickness, "m"),
                    quantity("GrossArea", object.plan[0].area(), "m2"),
                    quantity("GrossVolume", volume, "m3"),
                ];
            } else {
                object.id = format!(
                    "corridor/{}/{side_word}/{}/{region}",
                    built.name, component.name
                );
                object.name = format!("{} {side_word} {}", built.name, component.name);
                object.kind = String::from("kerb");
                // IfcKerb has no predefined type of its own to name.
                object.ifc_class = String::from("IfcKerb");
                object.predefined_type.clear();
                object.material = component.material.clone();
                object.colour = component.colour;
                object
                    .properties
                    .push((String::from("shape"), String::from(component.shape.name())));
                // IfcKerb's Mountable attribute.
                object.properties.push((
                    String::from("mountable"),
                    String::from(if component.shape == KerbShape::Mountable {
                        "true"
                    } else {
                        "false"
                    }),
                ));
                // A kerb's Height is its face; an edge strip is flush and has
                // none, so it reports how thick it is instead.
                object.quantities = vec![
                    quantity("Length", plan_length(alignment, &stations, &columns), "m"),
                    quantity("Width", kerb_width(component), "m"),
                    if component.shape == KerbShape::EdgeStrip {
                        quantity("Depth", component.depth, "m")
                    } else {
                        quantity("Height", component.height, "m")
                    },
                    quantity("GrossVolume", volume, "m3"),
                ];
            }
            object.mesh = mesh;
            model.objects.push(object);
        }

        // ---- earthworks -----------------------------------------------------------------
        for side in [Side::Left, Side::Right] {
            for cutting in [true, false] {
                add_earthworks(&mut model, built, alignment, run, side, cutting);
            }
        }
    }
    model
}

// ---- strings and surfaces ----------------------------------------------------------------

/// The sections of every run in station order. Where two runs meet - a
/// region's ends - both have a section at the station, the earlier run's
/// first: the templates differ there, so each run keeps its own section
/// (RoadSection::run tells them apart), and what is made from the list does
/// not blend one into the other across the interval beside the join.
pub fn corridor_sections(built: &BuiltCorridor) -> Vec<&RoadSection> {
    built
        .runs
        .iter()
        .flat_map(|run| run.sections.iter())
        .collect()
}

// A section and the station its points are laid at along the alignment.
struct Placed<'a> {
    section: &'a RoadSection,
    station: f64,
}

// The sections where they stand, or - `step` - with the first section of each
// run that meets the one before at one station laid FACE_RUN further on (no
// further than a quarter of the way to its run's next section). A surface
// holds one level at a point: two runs' sections at one station would make
// the TIN keep one of them, or blend them over the interval beside the join; a
// millimetre apart (more than the TIN's duplicate tolerance) they make a
// near-vertical face, the step a change of template is.
#[allow(
    clippy::needless_range_loop,
    reason = "the index names a section and its neighbours on both sides"
)]
fn placed_sections<'a>(sections: &[&'a RoadSection], step: bool) -> Vec<Placed<'a>> {
    let mut out = Vec::with_capacity(sections.len());
    for i in 0..sections.len() {
        let mut station = sections[i].station;
        if step
            && i > 0
            && sections[i].run != sections[i - 1].run
            && (station - sections[i - 1].station).abs() <= tolerance::GEOMETRIC
        {
            let mut room = FACE_RUN;
            if i + 1 < sections.len() && sections[i + 1].run == sections[i].run {
                room = cpp::min(room, 0.25 * (sections[i + 1].station - station));
            }
            station = sections[i - 1].station + room;
        }
        out.push(Placed {
            section: sections[i],
            station,
        });
    }
    out
}

/// One longitudinal string of the finished surface (or the formation): the
/// points of one code in consecutive sections.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CorridorString {
    pub code: String,
    pub points: Vec<Point3>,
}

fn close(finished: &mut Vec<CorridorString>, string: CorridorString) {
    if string.points.len() >= 2 {
        finished.push(string);
    }
}

// Strings by code over `placed`: a code missing from a section, a section not
// taken (`take` false) and the join of two runs end the strings through it.
fn strings_over(
    placed: &[Placed<'_>],
    alignment: &SolvedAlignment,
    formation: bool,
    take: impl Fn(&RoadSection) -> bool,
) -> Vec<CorridorString> {
    let mut finished = Vec::new();
    let mut open: BTreeMap<String, CorridorString> = BTreeMap::new();
    let mut before: Option<&RoadSection> = None;
    for at in placed {
        let section = at.section;
        let joined = before.is_some_and(|earlier| earlier.run != section.run);
        before = Some(section);
        if !take(section) || joined {
            for string in std::mem::take(&mut open).into_values() {
                close(&mut finished, string);
            }
            if !take(section) {
                continue;
            }
        }
        let line = if formation {
            &section.formation
        } else {
            &section.top
        };
        let mut next: BTreeMap<String, CorridorString> = BTreeMap::new();
        for point in line {
            let Some(plan) = alignment.point_at_station_offset(at.station, point.offset) else {
                continue;
            };
            let mut string = match open.remove(&point.code) {
                Some(found) => found,
                None => CorridorString {
                    code: point.code.clone(),
                    points: Vec::new(),
                },
            };
            string.points.push(Point3::new(plan.x, plan.y, point.level));
            next.entry(point.code.clone()).or_insert(string);
        }
        for string in std::mem::take(&mut open).into_values() {
            close(&mut finished, string);
        }
        open = next;
    }
    for string in open.into_values() {
        close(&mut finished, string);
    }
    finished
}

/// Every string, each broken where a section lacks its code and at every join
/// of two runs, so a string runs over its own run's stations and no further.
pub fn corridor_strings(built: &BuiltCorridor, formation: bool) -> Vec<CorridorString> {
    let Some(alignment) = &built.alignment else {
        return Vec::new();
    };
    strings_over(
        &placed_sections(&corridor_sections(built), false),
        alignment,
        formation,
        |section: &RoadSection| section.top.len() >= 2,
    )
}

/// The finished surface, or the formation, as a TIN: every section's points,
/// its strings as breaklines, and its outermost points as the boundary.
/// `complete_only` leaves the incomplete sections out, breaking the strings
/// there, and then uses no boundary if any was left out - what the old
/// corridor_surface did. Where two runs meet the later run's first section is
/// laid FACE_RUN (1 mm) further on and the strings break between them, so the
/// surface steps from one template to the next across the join - a TIN holds
/// no two levels at one point - and not over the interval beside it.
/// InvalidGeometry when fewer than two sections go in (naming how many were
/// incomplete), else whatever build_tin reports.
#[derive(Clone, Debug, Default)]
pub struct SectionsSurface {
    pub surface: TinSurface,
    pub sections: usize,
    pub left_out: usize,
}

pub fn sections_surface(
    sections: &[&RoadSection],
    alignment: &SolvedAlignment,
    formation: bool,
    complete_only: bool,
) -> Result<SectionsSurface> {
    let mut result = SectionsSurface::default();
    let mut input = TinInput::default();
    let mut left_edge: Vec<Point2> = Vec::new();
    let mut right_edge: Vec<Point2> = Vec::new();
    // A section goes in when it is complete (or completeness is not asked
    // for) and the line it gives has two points at least.
    let take = |section: &RoadSection| -> bool {
        let line = if formation {
            &section.formation
        } else {
            &section.top
        };
        (!complete_only || section.complete) && line.len() >= 2
    };
    let placed = placed_sections(sections, true);
    for entry in &placed {
        let section = entry.section;
        if !take(section) {
            result.left_out += 1;
            continue;
        }
        let line = if formation {
            &section.formation
        } else {
            &section.top
        };
        for point in line {
            if let Some(plan) = alignment.point_at_station_offset(entry.station, point.offset) {
                input.points.push(Point3::new(plan.x, plan.y, point.level));
            }
        }
        if let Some(first) = alignment.point_at_station_offset(entry.station, line[0].offset) {
            left_edge.push(first);
        }
        if let Some(last) =
            alignment.point_at_station_offset(entry.station, line[line.len() - 1].offset)
        {
            right_edge.push(last);
        }
        result.sections += 1;
    }
    if result.sections < 2 {
        // Only a surface of complete sections leaves one out for not reaching
        // the ground; otherwise it is the sections there are that are too few.
        return Err(make_error(
            ErrorCode::InvalidGeometry,
            if complete_only {
                "fewer than two sections reach the ground: there is no surface to build"
            } else {
                "fewer than two sections have a line to triangulate: there is no surface to build"
            },
            format!(
                "{}{}",
                result.left_out,
                if complete_only {
                    " incomplete"
                } else {
                    " left out"
                }
            ),
        ));
    }
    for string in strings_over(&placed, alignment, formation, take) {
        input.breaklines.push(Breakline {
            vertices: string.points,
            ..Breakline::default()
        });
    }
    // The two sections of a join are lines the triangulation must keep. The
    // strings end at the join, and the only constraint left across it would be
    // the Delaunay rule: the first section of the later run lies a millimetre
    // on, its points inside the circles of the thin triangles between the
    // earlier run's last two sections, so the TIN joined the last interval to
    // the other template and blended the two over it (9.793 at 6 m out and 35
    // m on the rural-urban road, where the rural surface is 9.585). With each
    // section a breakline the interval before the join is the earlier run's
    // alone, the 1 mm beyond it the face, and the later run its own.
    let cross_line = |at: &Placed<'_>| -> Breakline {
        let mut breakline = Breakline::default();
        let line = if formation {
            &at.section.formation
        } else {
            &at.section.top
        };
        for point in line {
            let plan = alignment.point_at_station_offset(at.station, point.offset);
            // A vertical face (two levels at one offset) is one place in plan.
            if let Some(plan) = plan
                && !breakline.vertices.last().is_some_and(|last| {
                    (plan.x - last.x).hypot(plan.y - last.y) <= tolerance::GEOMETRIC
                })
            {
                breakline
                    .vertices
                    .push(Point3::new(plan.x, plan.y, point.level));
            }
        }
        breakline
    };
    for pair in placed.windows(2) {
        let before = pair[0].section;
        let after = pair[1].section;
        if before.run == after.run
            || (before.station - after.station).abs() > tolerance::GEOMETRIC
            || !take(before)
            || !take(after)
        {
            continue;
        }
        for at in pair {
            let breakline = cross_line(at);
            if breakline.vertices.len() >= 2 {
                input.breaklines.push(breakline);
            }
        }
    }
    // A ring with gaps in it is not a ring: with sections left out the surface
    // is bounded by its hull instead (docs/cad.md).
    if result.left_out == 0 {
        input.boundary.vertices = left_edge;
        input.boundary.vertices.extend(right_edge.iter().rev());
        input.boundary.closed = true;
    }
    // Points and string vertices coincide exactly by construction, with equal
    // levels; strings that cross on the inside of a tight curve take the mean
    // of their levels (docs/cad.md).
    let options = TinBuildOptions {
        crossing_breaklines: CrossingBreaklinePolicy::Average,
        ..TinBuildOptions::default()
    };
    let built = build_tin(&input, &options)?;
    result.surface = built.surface;
    Ok(result)
}
