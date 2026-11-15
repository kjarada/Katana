//! Profiles and cross sections (PLAN.MD Phases 14 and 21;
//! include/katana/cad/section.hpp).
//!
//! Both are the same operation: sample one or more surfaces along a plan
//! alignment and report elevation against distance along it. A long section
//! runs down a road centreline; a cross section runs across it at one station,
//! on a line this module also builds. Everything here is headless and unit
//! tested - the section VIEW just draws what these return (Rule 3).
//!
//! SAMPLING. A fixed interval alone is not good enough for engineering: it cuts
//! the corners off every ridge and every breakline, so a volume taken from the
//! profile disagrees with one taken from the surface. Samples are therefore
//! placed at
//!
//! * every alignment vertex, because the alignment itself bends there;
//! * every interval along each segment;
//! * every point where the alignment crosses a TIN triangle edge, which is
//!   where the surface itself changes slope.
//!
//! The crossings are found by bisecting between consecutive samples whose
//! located triangle differs, rather than by walking the triangulation. The walk
//! is asymptotically better, but bisection needs nothing beyond TinSurface's
//! public point location, converges to GEOMETRIC in about forty iterations of
//! an O(1) query, and cannot be derailed by the surface having holes or by the
//! alignment leaving and re-entering it - all three of which happen constantly
//! in real survey data. Revisit with a profiler, not with intuition (Rule 6).

use katana_core::error::make_error;
use katana_core::text::format_exact_real;
use katana_core::{Error, ErrorCode, Result, Status};
use katana_entity::curve_pieces::curve_pieces;
use katana_entity::entity_geometry::bounding_box;
use katana_entity::{Entity, EntityId, Geometry, Model};
use katana_geometry::curves2d::CURVE_CHORD_TOLERANCE;
use katana_geometry::editing::bounding_box as curve_bounding_box;
use katana_geometry::intersection::intersect;
use katana_geometry::{
    Box2, Curve2, IntersectionKind, IntersectionResult, Point2, Polyline2, Segment2, SolvedProfile,
    SpatialIndex, Vec2,
};
use katana_math::cpp;
use katana_math::numerics::tolerance;
use katana_terrain::{NO_TRIANGLE, TinSurface};

use crate::layer_overrides::NO_LAYER_OVERRIDES;
use crate::selection::is_drawn;
use crate::spatial_query::worth_indexing;

/// One station on the section.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SectionSample {
    /// Distance along the alignment from its start.
    pub station: f64,
    /// The world XY position of that station.
    pub plan: Point2,
    /// None where the alignment is off the surface, in a hole, or past its
    /// rim. A gap in the profile is information, not an error, so it is
    /// represented rather than interpolated across.
    pub elevation: Option<f64>,
}

/// Why a sample was placed. Useful to the view, which draws breaks
/// differently from interval ticks, and to a caller exporting the profile.
/// ProfileVertex marks a PVC, PVT, PVI, high or low point of a design profile:
/// the stations a design is labelled and set out at.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SampleReason {
    #[default]
    Interval,
    AlignmentVertex,
    SurfaceBreak,
    Start,
    End,
    ProfileVertex,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SectionSurface {
    pub name: String,
    /// Ascending station, no duplicates.
    pub samples: Vec<SectionSample>,
    /// Parallel to samples.
    pub reasons: Vec<SampleReason>,
    /// Over the samples that have an elevation; None when none do, which is
    /// what an alignment that misses the surface entirely produces.
    pub min_elevation: Option<f64>,
    pub max_elevation: Option<f64>,
}

/// Where a drawing entity crosses the section line in plan. A pipe, a kerb or
/// a boundary drawn in 2D shows on the section at the station it crosses.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SectionCrossing {
    pub entity: EntityId,
    pub layer: String,
    pub station: f64,
    pub plan: Point2,
    /// Elevation taken from the entity's own geometry where it carries one,
    /// otherwise from the first surface that covers the crossing.
    pub elevation: Option<f64>,
}

#[derive(Clone, Copy, Debug)]
pub struct SectionOptions<'a> {
    /// Interval along the alignment, in model units. Must be positive.
    pub interval: f64,
    /// Add a sample wherever the alignment crosses a triangle edge.
    pub include_surface_breaks: bool,
    /// Include entities from the model that cross the alignment in plan.
    pub include_crossings: bool,
    /// Caps the number of samples per surface so that a 20 km alignment at a
    /// 1 mm interval fails cleanly instead of exhausting memory.
    pub maximum_samples: usize,
    /// The model's spatial index keyed by EntityId (Document::spatial_index)
    /// when the caller has one. The crossing search then asks it for the
    /// entities near the alignment instead of walking the whole model, which
    /// is what makes a short cross section through a large drawing cheap. The
    /// crossings are the same with or without it (the section tests hold both
    /// paths to one hand-worked answer); without it every entity is visited
    /// and rejected by its bounding box before any intersection is computed.
    pub spatial_index: Option<&'a SpatialIndex>,
}

impl Default for SectionOptions<'_> {
    fn default() -> Self {
        Self {
            interval: 1.0,
            include_surface_breaks: true,
            include_crossings: true,
            maximum_samples: 2_000_000,
            spatial_index: None,
        }
    }
}

/// A named surface to cut. Nothing is copied; None is refused.
#[derive(Clone, Copy, Debug)]
pub struct SectionSurfaceInput<'a> {
    pub name: &'a str,
    pub surface: Option<&'a TinSurface>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Section {
    pub alignment: Polyline2,
    pub length: f64,
    pub surfaces: Vec<SectionSurface>,
    pub crossings: Vec<SectionCrossing>,
}

impl Section {
    /// Station range x elevation range over everything sampled. Empty when no
    /// sample anywhere had an elevation - which is what a viewport needs in
    /// order to frame the drawing, and what tells it there is nothing to
    /// frame.
    pub fn extent(&self) -> Box2 {
        let mut extent = Box2::default();
        for surface in &self.surfaces {
            for sample in &surface.samples {
                if let Some(elevation) = sample.elevation {
                    extent.expand(Point2::new(sample.station, elevation));
                }
            }
        }
        for crossing in &self.crossings {
            if let Some(elevation) = crossing.elevation {
                extent.expand(Point2::new(crossing.station, elevation));
            }
        }
        extent
    }
}

/// The design profile cut beside the ground in a section: the solved profile
/// and the name its series carries. An open section view keeps it, with the
/// section's own alignment, as the recipe that cuts the section again when the
/// drawing's surfaces change (terrain's section_cut, cut_drawing_section).
#[derive(Clone, Debug, Default)]
pub struct SectionDesign {
    pub profile: SolvedProfile,
    pub name: String,
}

/// Cumulative length at each vertex, so a station converts to a position in
/// one binary search rather than by walking the polyline.
struct Stationing {
    /// at_vertex[0] == 0
    at_vertex: Vec<f64>,
    length: f64,
}

impl Stationing {
    fn position_at(&self, line: &Polyline2, station: f64) -> Point2 {
        if self.at_vertex.len() < 2 {
            return line.vertices.first().copied().unwrap_or_default();
        }
        let clamped = cpp::clamp(station, 0.0, self.length);
        // upper_bound then step back: the segment containing `clamped` is the
        // one whose start station is the greatest not exceeding it.
        let mut index = self.at_vertex.partition_point(|&at| !(clamped < at));
        if index == 0 {
            index = 1;
        }
        if index >= self.at_vertex.len() {
            index = self.at_vertex.len() - 1;
        }
        let from = self.at_vertex[index - 1];
        let to = self.at_vertex[index];
        let span = to - from;
        let a = line.vertices[index - 1];
        let b = line.vertices[index % line.vertices.len()];
        if span <= 0.0 {
            return a;
        }
        let t = (clamped - from) / span;
        a + (b - a) * t
    }
}

fn station_of(line: &Polyline2) -> Stationing {
    let mut at_vertex = Vec::with_capacity(line.vertices.len() + 1);
    let mut running = 0.0;
    at_vertex.push(0.0);
    for pair in line.vertices.windows(2) {
        running += pair[0].distance_to(pair[1]);
        at_vertex.push(running);
    }
    if line.closed && line.vertices.len() > 2 {
        running += line.vertices[line.vertices.len() - 1].distance_to(line.vertices[0]);
        at_vertex.push(running);
    }
    Stationing {
        at_vertex,
        length: running,
    }
}

/// The triangle a position sits in, or NO_TRIANGLE. Used only to detect that
/// the alignment has crossed from one triangle to another; the elevation
/// itself comes from elevation_at.
fn triangle_at(surface: &TinSurface, position: Point2) -> u32 {
    surface
        .locate(position)
        .map_or(NO_TRIANGLE, |location| location.triangle)
}

#[derive(Clone, Copy, Debug)]
struct Candidate {
    station: f64,
    reason: SampleReason,
}

/// Bisects [lo, hi] for the station at which the located triangle changes,
/// stopping once the bracket is below GEOMETRIC. Returns the station just past
/// the change, so that sampling there lands in the NEW triangle.
fn find_break(
    surface: &TinSurface,
    stationing: &Stationing,
    line: &Polyline2,
    mut lo: f64,
    mut hi: f64,
    lo_triangle: u32,
) -> f64 {
    // Bounded independently of the tolerance so a pathological surface cannot
    // spin here: 60 halvings takes any realistic bracket below 1e-15.
    let mut iteration = 0;
    while iteration < 60 && hi - lo > tolerance::GEOMETRIC {
        let mid = 0.5 * (lo + hi);
        if triangle_at(surface, stationing.position_at(line, mid)) == lo_triangle {
            lo = mid;
        } else {
            hi = mid;
        }
        iteration += 1;
    }
    hi
}

fn summarise(surface: &mut SectionSurface) {
    let mut low = f64::INFINITY;
    let mut high = f64::NEG_INFINITY;
    let mut any = false;
    for sample in &surface.samples {
        if let Some(elevation) = sample.elevation {
            low = cpp::min(low, elevation);
            high = cpp::max(high, elevation);
            any = true;
        }
    }
    if any {
        surface.min_elevation = Some(low);
        surface.max_elevation = Some(high);
    }
}

/// Exact plan crossings of `piece` with one entity's geometry.
///
/// The curve overloads of intersect() are used directly rather than
/// tessellating first: an arc crossing the alignment has ONE true station, and
/// a chorded approximation of it would put the crossing wherever the chords
/// happened to fall. Text, points and dimensions carry no plan curve and cross
/// nothing.
fn crossings_of(piece: &Segment2, geometry: &Geometry) -> IntersectionResult {
    match geometry {
        Geometry::Line(shape) => intersect(piece, shape),
        Geometry::Circle(shape) => intersect(piece, shape),
        Geometry::Arc(shape) => intersect(piece, shape),
        _ => IntersectionResult::default(),
    }
}

fn station_sort(candidates: &mut [Candidate]) {
    candidates.sort_by(|a, b| {
        a.station
            .partial_cmp(&b.station)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
}

/// Cuts `surfaces` along `alignment`.
///
/// Fails with InvalidArgument for an alignment of fewer than two distinct
/// vertices, a non-positive or non-finite interval, a missing surface, or a
/// request that would exceed `maximum_samples`.
pub fn extract_section(
    alignment: &Polyline2,
    surfaces: &[SectionSurfaceInput<'_>],
    model: Option<&Model>,
    options: &SectionOptions<'_>,
) -> Result<Section> {
    let cleaned = alignment.without_duplicate_vertices();
    if cleaned.vertices.len() < 2 {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "a section alignment needs at least two distinct vertices",
        ));
    }
    if !options.interval.is_finite() || options.interval <= 0.0 {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the section interval must be positive",
        ));
    }
    let mut inputs: Vec<(&str, &TinSurface)> = Vec::with_capacity(surfaces.len());
    for input in surfaces {
        let Some(surface) = input.surface else {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "null surface in the section request",
                input.name,
            ));
        };
        inputs.push((input.name, surface));
    }

    let stationing = station_of(&cleaned);
    if stationing.length <= tolerance::GEOMETRIC {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the section alignment has no length",
        ));
    }
    // Checked before anything is allocated: a 20 km alignment at a 1 mm
    // interval is a typo, and it must say so rather than swap the machine out.
    let wanted = stationing.length / options.interval;
    if wanted + cleaned.vertices.len() as f64 > options.maximum_samples as f64 {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "the interval would produce more samples than the limit allows",
            format!("{} > {}", wanted as i64, options.maximum_samples),
        ));
    }

    let mut section = Section {
        alignment: cleaned.clone(),
        length: stationing.length,
        ..Section::default()
    };

    // Stations every surface shares: the interval grid plus the alignment's
    // own vertices. Built once because it does not depend on the surface.
    let mut base = vec![Candidate {
        station: 0.0,
        reason: SampleReason::Start,
    }];
    for &at in &stationing.at_vertex[1..stationing.at_vertex.len() - 1] {
        base.push(Candidate {
            station: at,
            reason: SampleReason::AlignmentVertex,
        });
    }
    let mut station = options.interval;
    while station < stationing.length {
        base.push(Candidate {
            station,
            reason: SampleReason::Interval,
        });
        station += options.interval;
    }
    base.push(Candidate {
        station: stationing.length,
        reason: SampleReason::End,
    });
    station_sort(&mut base);

    for &(name, surface) in &inputs {
        let mut out = SectionSurface {
            name: String::from(name),
            ..SectionSurface::default()
        };

        let mut stations = base.clone();
        if options.include_surface_breaks && !surface.empty() {
            // One pass over consecutive base stations: where the located
            // triangle differs, the alignment crossed an edge somewhere
            // between them, and bisection finds where.
            let mut breaks = Vec::new();
            let mut previous = triangle_at(
                surface,
                stationing.position_at(&cleaned, stations[0].station),
            );
            for pair in stations.windows(2) {
                let current =
                    triangle_at(surface, stationing.position_at(&cleaned, pair[1].station));
                if current != previous {
                    let at = find_break(
                        surface,
                        &stationing,
                        &cleaned,
                        pair[0].station,
                        pair[1].station,
                        previous,
                    );
                    breaks.push(Candidate {
                        station: at,
                        reason: SampleReason::SurfaceBreak,
                    });
                }
                previous = current;
            }
            stations.extend(breaks);
            station_sort(&mut stations);
        }

        out.samples.reserve(stations.len());
        out.reasons.reserve(stations.len());
        for candidate in &stations {
            // Duplicates arise where a break lands on an interval station or a
            // vertex; the earlier reason wins because Start/Vertex say more
            // about the sample than Interval does.
            if let Some(last) = out.samples.last()
                && candidate.station - last.station <= tolerance::GEOMETRIC
            {
                continue;
            }
            let plan = stationing.position_at(&cleaned, candidate.station);
            out.samples.push(SectionSample {
                station: candidate.station,
                plan,
                elevation: surface.elevation_at(plan),
            });
            out.reasons.push(candidate.reason);
        }
        summarise(&mut out);
        section.surfaces.push(out);
    }

    if let Some(model) = model.filter(|_| options.include_crossings) {
        section.crossings = crossings(&cleaned, &stationing, &inputs, model, options);
    }

    Ok(section)
}

/// The entities of `model` that cross `cleaned`, sorted by station.
fn crossings(
    cleaned: &Polyline2,
    stationing: &Stationing,
    inputs: &[(&str, &TinSurface)],
    model: &Model,
    options: &SectionOptions<'_>,
) -> Vec<SectionCrossing> {
    let mut found: Vec<SectionCrossing> = Vec::new();
    let record = |found: &mut Vec<SectionCrossing>,
                  entity: &Entity,
                  segment_index: usize,
                  along: &Segment2,
                  at: Point2| {
        let mut elevation = None;
        for &(_, surface) in inputs {
            elevation = surface.elevation_at(at);
            if elevation.is_some() {
                break;
            }
        }
        found.push(SectionCrossing {
            entity: entity.id,
            layer: entity.layer.clone(),
            station: stationing.at_vertex[segment_index] + along.start.distance_to(at),
            plan: at,
            elevation,
        });
    };

    // Bounding-box rejection before any intersection is computed. The search
    // used to intersect every alignment segment with every segment of every
    // entity in the model; a box test is four comparisons, and on a real
    // drawing almost every pair fails it.
    //
    // The boxes are grown by a margin because intersect() accepts a crossing
    // within GEOMETRIC of BOTH curves: a crossing point can lie up to
    // GEOMETRIC outside either exact box, so exact boxes could reject a pair
    // that intersect() would have reported. Ten times the tolerance covers
    // that and the rounding of the point itself; a generous margin costs
    // nothing, a tight one would lose crossings.
    let margin = 10.0 * tolerance::GEOMETRIC;
    let mut along_boxes = Vec::with_capacity(cleaned.segment_count());
    let mut reach = Box2::default();
    for s in 0..cleaned.segment_count() {
        let inflated = cleaned.segment(s).bounding_box().inflated(margin);
        along_boxes.push(inflated);
        reach.expand_box(inflated);
    }

    let mut visit = |entity: &Entity| {
        // The drawing system's kinds cross the section through their pieces
        // (entity's curve_pieces.rs): a curve polyline's segments and arcs
        // exactly, an ellipse's and a spline's chords to a millimetre - a
        // curve this code did not know of would otherwise be missing from the
        // section with nothing to say so.
        let polyline = match &entity.geometry {
            Geometry::Polyline(polyline) => Some(polyline),
            _ => None,
        };
        let pieced = matches!(
            entity.geometry,
            Geometry::CurvePolyline(_) | Geometry::Ellipse(_) | Geometry::Spline(_)
        );
        // Text, points and dimensions carry no plan curve and cross nothing
        // (crossings_of); leaving them out here also spares them the box
        // computation.
        if polyline.is_none()
            && !pieced
            && !matches!(
                entity.geometry,
                Geometry::Line(_) | Geometry::Circle(_) | Geometry::Arc(_)
            )
        {
            return;
        }
        // The document rule: a section is cut once and shared, so it must not
        // depend on which view was active. A view that hides a layer drops
        // that layer's crossings when it paints them.
        if !is_drawn(model, entity, &NO_LAYER_OVERRIDES) {
            return; // what the plan hides is not on the section either
        }
        let extent = bounding_box(&entity.geometry);
        if !extent.intersects(reach) {
            return;
        }
        let pieces: Vec<Curve2> = if pieced {
            curve_pieces(&entity.geometry, CURVE_CHORD_TOLERANCE)
        } else {
            Vec::new()
        };
        for (s, along_box) in along_boxes.iter().enumerate() {
            if !extent.intersects(*along_box) {
                continue;
            }
            let along = cleaned.segment(s);
            if pieced {
                for piece in &pieces {
                    if !curve_bounding_box(piece).intersects(*along_box) {
                        continue;
                    }
                    let crossing = intersect(&Curve2::Segment(along), piece);
                    if crossing.kind != IntersectionKind::Points {
                        continue;
                    }
                    for &point in &crossing.points[..crossing.count] {
                        record(&mut found, entity, s, &along, point);
                    }
                }
                continue;
            }
            if let Some(polyline) = polyline {
                // A long alignment's box covers most of the drawing, so for it
                // the box test rejects little. The sharper test: an edge with
                // both ends on the same side of the alignment's line, each
                // further than the margin from it, cannot come within
                // GEOMETRIC of it. The cross product is the distance times the
                // segment's length, hence the scaled threshold; its rounding
                // (about 1e-16 of the product of two lengths) is far inside the
                // margin.
                let direction: Vec2 = along.delta();
                let across = margin * direction.length();
                let side = |p: Point2| direction.cross(p - along.start);
                for i in 0..polyline.segment_count() {
                    let edge = polyline.segment(i);
                    if !edge.bounding_box().intersects(*along_box) {
                        continue;
                    }
                    let side_start = side(edge.start);
                    let side_end = side(edge.end);
                    if (side_start > across && side_end > across)
                        || (side_start < -across && side_end < -across)
                    {
                        continue;
                    }
                    let piece = intersect(&along, &edge);
                    if piece.kind != IntersectionKind::Points {
                        continue;
                    }
                    for &point in &piece.points[..piece.count] {
                        record(&mut found, entity, s, &along, point);
                    }
                }
                continue;
            }
            let hit = crossings_of(&along, &entity.geometry);
            if hit.kind != IntersectionKind::Points {
                continue; // an overlap has no single station to report
            }
            for &point in &hit.points[..hit.count] {
                record(&mut found, entity, s, &along, point);
            }
        }
    };

    // Entities in ascending id either way - the index returns its ids sorted,
    // and the model walks its map in key order - so the crossings reach the
    // stable sort below in the same order and ties at one station keep the
    // same order with or without the index.
    match options.spatial_index {
        Some(index) if worth_indexing(index, reach) => {
            for id in index.query(reach) {
                // A stale id (an index that outlived its entity) is skipped:
                // a narrower answer, never a dangling reference.
                if let Some(entity) = model.entities.find(id) {
                    visit(entity);
                }
            }
        }
        _ => model.entities.for_each(&mut visit),
    }
    found.sort_by(|a, b| {
        a.station
            .partial_cmp(&b.station)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    found
}

/// Builds the plan line of a cross section: perpendicular to `alignment` at
/// `station`, running from -half_width to +half_width offset. The returned
/// line runs left to right looking along the alignment, so a cross section
/// drawn from it reads the way a surveyor expects.
///
/// Fails with InvalidArgument when the station is outside the alignment, when
/// half_width is not positive, or when the alignment has no direction there.
#[allow(
    clippy::manual_range_contains,
    reason = "The C++'s comparisons, kept as they are written."
)]
pub fn cross_section_line(
    alignment: &Polyline2,
    station: f64,
    half_width: f64,
) -> Result<Polyline2> {
    let cleaned = alignment.without_duplicate_vertices();
    if cleaned.vertices.len() < 2 {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "a cross section needs an alignment of at least two distinct vertices",
        ));
    }
    if !half_width.is_finite() || half_width <= 0.0 {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the cross section width must be positive",
        ));
    }
    let stationing = station_of(&cleaned);
    if !station.is_finite()
        || station < -tolerance::GEOMETRIC
        || station > stationing.length + tolerance::GEOMETRIC
    {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "the station is outside the alignment",
            format!(
                "{} of {}",
                format_exact_real(station),
                format_exact_real(stationing.length)
            ),
        ));
    }

    let clamped = cpp::clamp(station, 0.0, stationing.length);
    let centre = stationing.position_at(&cleaned, clamped);

    // Direction from a short chord about the station rather than from the
    // segment alone, so a station landing exactly on a vertex has a defined
    // direction (the bisector) instead of depending on which side it rounds
    // to.
    let reach = cpp::max(stationing.length * 1.0e-6, tolerance::GEOMETRIC);
    let behind = stationing.position_at(&cleaned, cpp::max(0.0, clamped - reach));
    let ahead = stationing.position_at(&cleaned, cpp::min(stationing.length, clamped + reach));
    let direction = ahead - behind;
    let length = direction.length();
    if length <= tolerance::GEOMETRIC {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the alignment has no direction at that station",
        ));
    }
    let unit = direction / length;
    // Left of the direction of travel is (-dy, dx); the line runs left to
    // right so that offsets increase to the right, as a surveyor reads a
    // section.
    let normal = Vec2::new(-unit.y, unit.x);

    Ok(Polyline2::new(
        vec![centre + normal * half_width, centre - normal * half_width],
        false,
    ))
}

/// Stations at a regular interval along `alignment`, always including 0 and
/// the full length. The set of cross sections a corridor is cut at.
pub fn section_stations(alignment: &Polyline2, interval: f64) -> Result<Vec<f64>> {
    let cleaned = alignment.without_duplicate_vertices();
    if cleaned.vertices.len() < 2 {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "stationing needs an alignment of at least two distinct vertices",
        ));
    }
    if !interval.is_finite() || interval <= 0.0 {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the stationing interval must be positive",
        ));
    }
    let length = station_of(&cleaned).length;
    if length <= tolerance::GEOMETRIC {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the alignment has no length",
        ));
    }
    if length / interval > 1.0e7 {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the interval would produce an unreasonable number of stations",
        ));
    }

    let mut stations = Vec::with_capacity((length / interval) as usize + 2);
    // Multiplied rather than accumulated, so the last station is not the sum
    // of ten thousand roundings.
    let mut i: usize = 0;
    loop {
        let station = i as f64 * interval;
        if station >= length - tolerance::GEOMETRIC {
            break;
        }
        stations.push(station);
        i += 1;
    }
    stations.push(length);
    Ok(stations)
}

/// The world position `distance` along `line`, walking its segments. The
/// section keeps the chorded alignment it was cut along, so this is what puts
/// a design station back on the plan for the cursor readout.
fn point_along(line: &Polyline2, distance: f64) -> Point2 {
    if line.vertices.is_empty() {
        return Point2::default();
    }
    let mut remaining = cpp::max(distance, 0.0);
    let count = line.vertices.len();
    for (i, pair) in line.vertices.windows(2).enumerate() {
        let a = pair[0];
        let b = pair[1];
        let length = a.distance_to(b);
        if remaining <= length || i + 2 == count {
            let t = if length > 0.0 {
                cpp::min(remaining / length, 1.0)
            } else {
                0.0
            };
            return a + (b - a) * t;
        }
        remaining -= length;
    }
    line.vertices[count - 1]
}

/// Adds the design grade line as one more series of `section`, named `name`,
/// so that whatever draws the ground draws the design beside it and the cut
/// and fill between them is visible.
///
/// Sampled at every station any existing series already has - so design and
/// ground can be read against each other at the same stations - plus the
/// profile's key stations and high and low points, marked ProfileVertex.
/// Stations the profile does not cover get no elevation, which the view draws
/// as a gap: a design shorter than the section shows as exactly that.
///
/// Fails with InvalidArgument for an empty name or a section of no length.
#[allow(
    clippy::manual_range_contains,
    reason = "The C++ erases a station below 0 or past the end and keeps a NaN, which a range's contains would not."
)]
pub fn append_design_profile(
    section: &mut Section,
    profile: &SolvedProfile,
    name: String,
) -> Status {
    if name.is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "a design profile needs a name",
        ));
    }
    if !(section.length > 0.0) {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the section has no length to profile",
        ));
    }

    // Stations: everything already sampled, plus the profile's own key
    // stations and extrema, within the section.
    let mut stations: Vec<(f64, SampleReason)> = Vec::new();
    for surface in &section.surfaces {
        for sample in &surface.samples {
            stations.push((sample.station, SampleReason::Interval));
        }
    }
    for station in profile.key_stations() {
        stations.push((station, SampleReason::ProfileVertex));
    }
    for point in profile.high_low_points() {
        stations.push((point.station, SampleReason::ProfileVertex));
    }
    if section.surfaces.is_empty() {
        // Nothing to align with: sample the design on its own at the interval
        // the window uses for ground, so the curve is drawn as a curve.
        let interval = cpp::max(section.length / 2000.0, 0.1);
        let mut station = 0.0;
        while station < section.length {
            stations.push((station, SampleReason::Interval));
            station += interval;
        }
        stations.push((section.length, SampleReason::End));
    }
    stations.retain(|entry| !(entry.0 < 0.0 || entry.0 > section.length));
    // std::sort, not stable: entries of one station may come out in either
    // order, and the collapse below gives the same result for both.
    stations.sort_unstable_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    // Duplicates collapse to one sample; a ProfileVertex wins over an Interval
    // at the same station because it carries more meaning.
    let mut unique: Vec<(f64, SampleReason)> = Vec::new();
    for entry in stations {
        if let Some(last) = unique.last_mut()
            && (last.0 - entry.0).abs() < 1e-9
        {
            if entry.1 == SampleReason::ProfileVertex {
                last.1 = entry.1;
            }
            continue;
        }
        unique.push(entry);
    }

    let mut design = SectionSurface {
        name,
        ..SectionSurface::default()
    };
    design.samples.reserve(unique.len());
    design.reasons.reserve(unique.len());
    for (station, reason) in unique {
        let sample = SectionSample {
            station,
            plan: point_along(&section.alignment, station),
            elevation: profile.elevation_at(station),
        };
        if let Some(elevation) = sample.elevation {
            design.min_elevation = Some(match design.min_elevation {
                Some(low) => cpp::min(low, elevation),
                None => elevation,
            });
            design.max_elevation = Some(match design.max_elevation {
                Some(high) => cpp::max(high, elevation),
                None => elevation,
            });
        }
        design.samples.push(sample);
        design.reasons.push(reason);
    }
    section.surfaces.push(design);
    Ok(())
}
