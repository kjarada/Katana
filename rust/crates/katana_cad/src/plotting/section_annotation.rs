//! What a section says besides its lines (docs/plotting.md, "Section smarts";
//! include/katana/cad/plotting/section_annotation.hpp): the cut and fill
//! between the design and the ground, the level and depth of each thing it
//! crosses, and where every label goes so that none is drawn over another or
//! outside its plot.
//!
//! Pure functions on a cut section (cad's section.rs) and on paper
//! millimetres: the painter measures its text and draws what these decide, so
//! each decision is tested without a pixel and an agent can read the same
//! numbers the sheet prints.

use std::cmp::Ordering;

use katana_entity::entity::heights_of;
use katana_entity::{Geometry, Model};
use katana_geometry::{Box2, Point2};
use katana_math::cpp;

use crate::layer_overrides::LayerOverrides;
use crate::plotting::sheet_set::std_fixed;
use crate::section::{Section, SectionCrossing, SectionSurface};

// Written to two decimals, as a drafter writes a service's level and depth;
// a value that rounds to nothing is "0.00", never "-0.00".
fn two_decimals(value: f64) -> String {
    std_fixed(if value.abs() < 0.005 { 0.0 } else { value }, 2)
}

fn between(a: Option<f64>, b: Option<f64>, t: f64) -> Option<f64> {
    let (a, b) = (a?, b?);
    Some(a + t * (b - a))
}

// Two paper boxes closer than `gap` on both axes.
fn crowds(a: &Box2, b: &Box2, gap: f64) -> bool {
    a.min.x < b.max.x + gap
        && b.min.x < a.max.x + gap
        && a.min.y < b.max.y + gap
        && b.min.y < a.max.y + gap
}

fn inside(bounds: &Box2, rect: &Box2) -> bool {
    const SLACK: f64 = 1e-9;
    !rect.empty()
        && rect.min.x >= bounds.min.x - SLACK
        && rect.max.x <= bounds.max.x + SLACK
        && rect.min.y >= bounds.min.y - SLACK
        && rect.max.y <= bounds.max.y + SLACK
}

// ---- the series ------------------------------------------------------------------------

/// A series is the design when its name starts with "design", in any case:
/// the long section's grade line ("design ROAD") or a design surface
/// ("DESIGN FINISHED SURFACE"). Every other series is ground.
pub fn is_design_series(name: &str) -> bool {
    let bytes = name.as_bytes();
    bytes.len() >= 6 && bytes[..6].eq_ignore_ascii_case(b"DESIGN")
}

/// The section's first design series. None when it has none.
pub fn design_series(section: &Section) -> Option<&SectionSurface> {
    section
        .surfaces
        .iter()
        .find(|surface| is_design_series(&surface.name))
}

/// The section's first other series - the ground that cut, fill and depth are
/// measured from. None when it has none.
pub fn ground_series(section: &Section) -> Option<&SectionSurface> {
    section
        .surfaces
        .iter()
        .find(|surface| !is_design_series(&surface.name))
}

/// How far past its first or last sample a station still reads that end: a
/// micrometre, so a range whose end is worked out from the section's length
/// is not a gap at its last column.
pub const SECTION_END_SLACK: f64 = 1e-6;

/// The level of `surface` at `station`, straight between its samples;
/// nothing in a gap or past either end (by more than SECTION_END_SLACK).
pub fn level_at(surface: &SectionSurface, station: f64) -> Option<f64> {
    let samples = &surface.samples;
    let (first, last) = (samples.first()?, samples.last()?);
    // A station a hair past an end - a range's end worked out from the
    // section's length - reads that end.
    if (station - last.station).abs() <= SECTION_END_SLACK {
        return last.elevation;
    }
    if (station - first.station).abs() <= SECTION_END_SLACK {
        return first.elevation;
    }
    let after = samples.partition_point(|sample| sample.station < station);
    let at = samples.get(after)?;
    if at.station == station {
        return at.elevation;
    }
    if after == 0 {
        return None;
    }
    let before = &samples[after - 1];
    let t = (station - before.station) / (at.station - before.station);
    between(before.elevation, at.elevation, t)
}

/// The levels a section has between stations `from` and `to` - every series'
/// samples and every crossing's level (crossing_note), with the series
/// interpolated at the two ends - as (lowest, highest); nothing when none has
/// a level there. `model` gives the crossings their own levels; a crossing on
/// a layer `hidden` hides is not drawn, so it does not count.
pub fn level_range(
    section: &Section,
    from: f64,
    to: f64,
    model: Option<&Model>,
    hidden: Option<&LayerOverrides>,
) -> Option<(f64, f64)> {
    let mut low = f64::INFINITY;
    let mut high = f64::NEG_INFINITY;
    let mut take = |level: Option<f64>| {
        if let Some(level) = level
            && level.is_finite()
        {
            low = cpp::min(low, level);
            high = cpp::max(high, level);
        }
    };
    for surface in &section.surfaces {
        for sample in &surface.samples {
            if sample.station >= from && sample.station <= to {
                take(sample.elevation);
            }
        }
        // A range that ends between two samples ends on the line between them.
        take(level_at(surface, from));
        take(level_at(surface, to));
    }
    for crossing in &section.crossings {
        if crossing.station >= from
            && crossing.station <= to
            && hidden.is_none_or(|hidden| !hidden.hides(&crossing.layer))
        {
            take(crossing_note(section, crossing, model).level);
        }
    }
    if !(high >= low) {
        return None;
    }
    Some((low, high))
}

// ---- cut and fill ------------------------------------------------------------------------

/// Design less ground at `station`: positive is fill, negative is cut;
/// nothing where either has no level.
pub fn cut_fill_at(design: &SectionSurface, ground: &SectionSurface, station: f64) -> Option<f64> {
    let d = level_at(design, station)?;
    let g = level_at(ground, station)?;
    Some(d - g)
}

/// A cut or fill as a data band writes it: signed, three decimals, "+0.250",
/// "-1.200", "0.000".
pub fn cut_fill_text(design_less_ground: f64) -> String {
    if design_less_ground.abs() < 0.0005 {
        return String::from("0.000");
    }
    // std::format's "{:+.3f}": Rust writes its digits, but a NaN as "NaN".
    if design_less_ground.is_nan() {
        return String::from(if design_less_ground.is_sign_negative() {
            "-nan"
        } else {
            "+nan"
        });
    }
    format!("{design_less_ground:+.3}")
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Earthwork {
    #[default]
    Cut,
    Fill,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct EarthworkRegion {
    pub kind: Earthwork,
    /// (station, level), closed without repeating its first point: along
    /// the design from the region's first station to its last, then back
    /// along the ground.
    pub outline: Vec<Point2>,
}

// A region being built: its kind, and the design and ground points so far.
struct Open {
    kind: Earthwork,
    design: Vec<Point2>,
    ground: Vec<Point2>,
}

// One end of a piece: the station and the two levels there.
#[derive(Clone, Copy)]
struct End {
    station: f64,
    design: f64,
    ground: f64,
}

#[derive(Default)]
struct Regions {
    open: Option<Open>,
    regions: Vec<EarthworkRegion>,
}

impl Regions {
    fn close(&mut self) {
        if let Some(open) = self.open.take()
            && open.design.len() >= 2
        {
            let mut outline = open.design.clone();
            // Back along the ground, leaving out a point where the two meet:
            // it is the design's already.
            let count = open.ground.len();
            for (k, point) in open.ground.iter().enumerate().rev() {
                let meets_at_end = k + 1 == count && open.design.last() == Some(point);
                let meets_at_start = k == 0 && open.design.first() == Some(point);
                if !meets_at_end && !meets_at_start {
                    outline.push(*point);
                }
            }
            self.regions.push(EarthworkRegion {
                kind: open.kind,
                outline,
            });
        }
    }

    fn piece(&mut self, a: End, b: End) {
        let sum = (a.design - a.ground) + (b.design - b.ground);
        if sum == 0.0 {
            self.close(); // the two coincide: nothing to shade
            return;
        }
        let kind = if sum > 0.0 {
            Earthwork::Fill
        } else {
            Earthwork::Cut
        };
        if let Some(open) = &mut self.open
            && open.kind == kind
        {
            open.design.push(Point2::new(b.station, b.design));
            open.ground.push(Point2::new(b.station, b.ground));
            return;
        }
        self.close();
        self.open = Some(Open {
            kind,
            design: vec![
                Point2::new(a.station, a.design),
                Point2::new(b.station, b.design),
            ],
            ground: vec![
                Point2::new(a.station, a.ground),
                Point2::new(b.station, b.ground),
            ],
        });
    }
}

/// The regions between the design and the ground over [from, to], in
/// station order: CUT where the ground is above the design, FILL where it is
/// below. They are split where the two cross (at the crossing, found on the
/// straight lines between samples) and where either has a gap; pieces of one
/// kind that follow each other are one region. Where the two coincide there
/// is nothing to shade. The C++'s defaults for `from` and `to` are the
/// infinities.
pub fn earthwork_regions(
    design: &SectionSurface,
    ground: &SectionSurface,
    from: f64,
    to: f64,
) -> Vec<EarthworkRegion> {
    let (Some(design_first), Some(design_last)) = (design.samples.first(), design.samples.last())
    else {
        return Vec::new();
    };
    let (Some(ground_first), Some(ground_last)) = (ground.samples.first(), ground.samples.last())
    else {
        return Vec::new();
    };
    // Where both series are, within the range asked for.
    let first = cpp::max3(from, design_first.station, ground_first.station);
    let last = cpp::min3(to, design_last.station, ground_last.station);
    if !(last > first) {
        return Vec::new();
    }
    // Every station either series has a sample at: between two of them both
    // are straight, so where they cross is found exactly.
    let mut stations = vec![first, last];
    for series in [design, ground] {
        for sample in &series.samples {
            if sample.station > first && sample.station < last {
                stations.push(sample.station);
            }
        }
    }
    stations.sort_by(f64::total_cmp);
    stations.dedup();

    let mut regions = Regions::default();
    for pair in stations.windows(2) {
        let (s0, s1) = (pair[0], pair[1]);
        let levels = (
            level_at(design, s0),
            level_at(ground, s0),
            level_at(design, s1),
            level_at(ground, s1),
        );
        let (Some(d0), Some(g0), Some(d1), Some(g1)) = levels else {
            regions.close(); // a gap in either: the region ends at it
            continue;
        };
        let a = End {
            station: s0,
            design: d0,
            ground: g0,
        };
        let b = End {
            station: s1,
            design: d1,
            ground: g1,
        };
        let diff_a = a.design - a.ground;
        let diff_b = b.design - b.ground;
        if diff_a * diff_b < 0.0 {
            // They cross between the two: split there, the level the same.
            let t = diff_a / (diff_a - diff_b);
            let station = s0 + t * (s1 - s0);
            let level = a.design + t * (b.design - a.design);
            let cross = End {
                station,
                design: level,
                ground: level,
            };
            regions.piece(a, cross);
            regions.piece(cross, b);
        } else {
            regions.piece(a, b);
        }
    }
    regions.close();
    regions.regions
}

/// The area of a closed outline (shoelace), always positive: an earthwork
/// region's in square metres of section.
pub fn outline_area(outline: &[Point2]) -> f64 {
    if outline.len() < 3 {
        return 0.0;
    }
    // Relative to the first point, so a level of 1000 m costs no precision.
    let origin = outline[0];
    let mut twice = 0.0;
    for pair in outline[1..].windows(2) {
        let a = pair[0] - origin;
        let b = pair[1] - origin;
        twice += a.x * b.y - b.x * a.y;
    }
    f64::abs(twice) / 2.0
}

// ---- crossings ------------------------------------------------------------------------------

/// The level an entity carries where the section crosses it: its own heights
/// (entity's heights_of - a circle's one, a line's or string's at each vertex,
/// an arc's at its two ends) straight between the two either side of the
/// crossing. Nothing when it has none there: a line drawn flat has no level
/// of its own, and the section drapes it on the ground.
pub fn own_level(model: &Model, crossing: &SectionCrossing) -> Option<f64> {
    let entity = model.entities.find(crossing.entity)?;
    let properties = &entity.properties;
    match &entity.geometry {
        Geometry::Line(shape) => {
            let heights = heights_of(properties, 2);
            between(heights[0], heights[1], shape.parameter_of(crossing.plan))
        }
        Geometry::Polyline(shape) => {
            let count = shape.vertices.len();
            if shape.segment_count() == 0 {
                return None;
            }
            // The piece it crosses: the one nearest the crossing point.
            let mut nearest = 0;
            let mut nearest_distance = f64::INFINITY;
            for i in 0..shape.segment_count() {
                let distance = shape.segment(i).distance_to(crossing.plan);
                if distance < nearest_distance {
                    nearest = i;
                    nearest_distance = distance;
                }
            }
            let heights = heights_of(properties, count);
            between(
                heights[nearest],
                heights[(nearest + 1) % count],
                shape.segment(nearest).parameter_of(crossing.plan),
            )
        }
        Geometry::Arc(shape) => {
            let heights = heights_of(properties, 2);
            let radial = crossing.plan - shape.center;
            between(
                heights[0],
                heights[1],
                shape.parameter_of_angle(radial.y.atan2(radial.x)),
            )
        }
        Geometry::Circle(_) => heights_of(properties, 1)[0],
        _ => None,
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CrossingNote {
    /// Its own level, else the level the section found under it (the
    /// ground's); nothing when neither.
    pub level: Option<f64>,
    pub own_level: bool,
    /// Ground less its own level at the crossing: how deep it is buried,
    /// negative when it is above the ground (an overhead line). Only for an
    /// own level over ground: a draped line's is zero by definition.
    pub depth: Option<f64>,
    /// "WATER RL 24.10 D 1.20"; "POWER RL 36.00 H 6.00" above the ground;
    /// "KERB RL 23.95" draped; "FENCE" with no level. And the layer alone, for
    /// when the whole does not fit.
    pub text: String,
    pub short_text: String,
}

/// What is written where `crossing` cuts `section`: its layer, level and
/// depth, to two decimals as a drafter writes a service. `model` gives the
/// crossing its own level; without one every level is the ground's.
pub fn crossing_note(
    section: &Section,
    crossing: &SectionCrossing,
    model: Option<&Model>,
) -> CrossingNote {
    let mut note = CrossingNote {
        short_text: crossing.layer.to_ascii_uppercase(),
        ..CrossingNote::default()
    };
    if let Some(model) = model
        && let Some(own) = own_level(model, crossing)
    {
        note.level = Some(own);
        note.own_level = true;
        if let Some(ground) = ground_series(section)
            && let Some(surface) = level_at(ground, crossing.station)
        {
            note.depth = Some(surface - own);
        }
    }
    if !note.own_level {
        note.level = crossing.elevation;
    }
    note.text = note.short_text.clone();
    if let Some(level) = note.level {
        note.text.push_str(&format!(" RL {}", two_decimals(level)));
    }
    if let Some(depth) = note.depth {
        // Below the ground a depth; above it (an overhead line) a height.
        note.text.push_str(if depth >= 0.0 { " D " } else { " H " });
        note.text.push_str(&two_decimals(depth.abs()));
    }
    note
}

// ---- placing labels ---------------------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LabelCandidate {
    /// Where the label could go on the paper, best first. A label with a
    /// shorter form lists the long form's places, then the short form's.
    pub boxes: Vec<Box2>,
    /// Lower is placed first; the painter uses the distance from the plot's
    /// middle, so the labels nearest the centreline are the ones kept.
    pub priority: f64,
}

/// The C++'s default for place_labels' `gap_mm`.
pub const LABEL_GAP_MM: f64 = 0.3;

/// Places `labels` in priority order (ties in the order given): each takes the
/// first of its boxes that lies inside `bounds` and keeps `gap_mm` clear of
/// every obstacle and every label placed before it. The index of the box each
/// took; nothing for a label that has none free, which is not drawn - a label
/// is dropped rather than drawn over another or outside its plot.
pub fn place_labels(
    labels: &[LabelCandidate],
    bounds: &Box2,
    obstacles: &[Box2],
    gap_mm: f64,
) -> Vec<Option<usize>> {
    let mut chosen = vec![None; labels.len()];
    let mut order: Vec<usize> = (0..labels.len()).collect();
    // A stable sort, as the C++'s std::stable_sort; two priorities neither
    // below the other (a NaN) keep their order.
    order.sort_by(|&a, &b| {
        labels[a]
            .priority
            .partial_cmp(&labels[b].priority)
            .unwrap_or(Ordering::Equal)
    });
    let mut taken: Vec<Box2> = obstacles.to_vec();
    for index in order {
        for (k, candidate) in labels[index].boxes.iter().enumerate() {
            if !inside(bounds, candidate) {
                continue;
            }
            let clear = !taken.iter().any(|other| crowds(candidate, other, gap_mm));
            if clear {
                chosen[index] = Some(k);
                taken.push(*candidate);
                break;
            }
        }
    }
    chosen
}
