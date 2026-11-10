//! Contours into the drawing (include/katana/interop/geo/contour_entities.hpp,
//! src/katana_interop/geo/contour_entities.cpp, docs/terrain.md
//! "Contours"): the ONE place the two engines of CONTOUR meet. A surface's
//! contours come from the exact TIN tracer (katana_terrain::contours); a
//! raster's from GDAL's `raster contour`, whose features are turned into the
//! same katana_terrain::Contour here - so which contour is major, which layer
//! it goes to and what height it carries are decided once, whichever engine
//! traced it.
//!
//! ```text
//! contours_from_features  GDAL's contour lines (written with
//!                         --elevation-name and --3d) as Contour, major by
//!                         the tracer's own rule: level = base + k *
//!                         interval, major when k is a multiple of
//!                         major_every.
//! boundaries_of           the closed shapes a scope took, as the areas a
//!                         contour is kept inside.
//! clip_contours           the contours cut where they leave those areas.
//! contour_command         ONE command: polylines on <layer>/major and
//!                         <layer>/minor, rings closed, each carrying its
//!                         level as its height (set_heights: the
//!                         `elevation` property).
//! ```

use std::cmp::Ordering;

use katana_commands::{CommandPtr, Transaction, create_entities, create_layer};
use katana_core::error::make_error;
use katana_core::{Error, ErrorCode, Result};
use katana_entity::entity::set_heights;
use katana_entity::layer_path::validate_layer_path;
use katana_entity::{Entity, Geometry, Layer, Model};
use katana_geometry::intersection::intersect;
use katana_geometry::{Box2, Containment, Point2, Polyline2, Segment2};
use katana_gis::processing::{FeatureSet, FieldValue};
use katana_gis::{GeoPoint, GeometryKind};
use katana_terrain::Contour;

/// The field CONTOUR asks GDAL to write each line's level in: without
/// --elevation-name GDAL writes no level at all, only an ID.
pub const CONTOUR_ELEVATION_FIELD: &str = "elevation";

/// One area a contour is kept inside: its exterior ring and its holes, each
/// closed. A point is in it when it is inside the exterior and in no hole.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ContourBoundary {
    pub rings: Vec<Polyline2>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContourEntityOptions {
    /// Major contours go to <layer>/major, the others to <layer>/minor.
    pub layer: String,
    /// The transaction's name: the verb line, so the history says what ran.
    pub command_name: String,
}

impl Default for ContourEntityOptions {
    fn default() -> Self {
        Self {
            layer: String::from("terrain/contours"),
            command_name: String::new(),
        }
    }
}

#[derive(Default)]
pub struct ContourPlan {
    /// None when there are no contours: an empty transaction is refused by
    /// the command stack, and nothing to draw changes nothing.
    pub command: Option<CommandPtr>,
    pub created: usize,
    pub major: usize,
    pub minor: usize,
    /// Distinct levels with at least one contour.
    pub levels: usize,
    /// The layers written to, major first.
    pub layers: Vec<String>,
}

fn number_of(value: &FieldValue) -> Option<f64> {
    match value {
        FieldValue::Real(real) => Some(*real),
        FieldValue::Integer64(whole) => Some(*whole as f64),
        _ => None,
    }
}

/// The ring or string of plan points a part is: a ring GDAL closed by
/// repeating its first vertex is closed, and the repeat dropped (Polyline2
/// closes a ring itself).
fn line_of(points: &[GeoPoint]) -> Option<Polyline2> {
    let mut line = Polyline2 {
        vertices: points
            .iter()
            .map(|point| Point2::new(point.x, point.y))
            .collect(),
        closed: false,
    };
    if line.vertices.len() >= 4 && line.vertices.first() == line.vertices.last() {
        line.vertices.pop();
        line.closed = true;
    }
    if line.vertices.len() < 2 {
        return None;
    }
    Some(line)
}

fn box_of(line: &Polyline2) -> Box2 {
    let mut bounds = Box2::default();
    for &vertex in &line.vertices {
        bounds.expand(vertex);
    }
    bounds
}

/// A boundary's rings with their boxes, so a segment far from a ring costs
/// one box test and not one per edge.
struct Area<'a> {
    rings: Vec<&'a Polyline2>,
    boxes: Vec<Box2>,
    bounds: Box2,
}

fn areas_of(boundaries: &[ContourBoundary]) -> Vec<Area<'_>> {
    let mut areas = Vec::new();
    for boundary in boundaries {
        let mut area = Area {
            rings: Vec::new(),
            boxes: Vec::new(),
            bounds: Box2::default(),
        };
        for ring in &boundary.rings {
            if ring.vertices.len() < 3 {
                continue;
            }
            let ring_box = box_of(ring);
            area.rings.push(ring);
            area.boxes.push(ring_box);
            area.bounds.expand_box(ring_box);
        }
        if !area.rings.is_empty() {
            areas.push(area);
        }
    }
    areas
}

/// Inside the exterior and in no hole - by the parity of the rings around
/// the point, so a hole needs no flag - or on any ring.
fn inside(areas: &[Area<'_>], point: Point2) -> bool {
    for area in areas {
        if !area.bounds.contains(point) {
            continue;
        }
        let mut around = 0_usize;
        for (ring, ring_box) in area.rings.iter().zip(&area.boxes) {
            if !ring_box.contains(point) {
                continue;
            }
            let place = ring.classify(point);
            if place == Containment::OnBoundary {
                return true;
            }
            around += usize::from(place == Containment::Inside);
        }
        if around % 2 == 1 {
            return true;
        }
    }
    false
}

/// Where along `segment` (0 to 1) it crosses or touches a ring's edge, ends
/// excluded, ascending.
fn cuts_of(segment: &Segment2, areas: &[Area<'_>]) -> Vec<f64> {
    let along = segment.delta();
    let length2 = along.length_squared();
    let mut cuts = Vec::new();
    if !(length2 > 0.0) {
        return cuts;
    }
    let mut bounds = Box2::default();
    bounds.expand(segment.start);
    bounds.expand(segment.end);
    for area in areas {
        if !area.bounds.intersects(bounds) {
            continue;
        }
        for (ring, ring_box) in area.rings.iter().zip(&area.boxes) {
            if !ring_box.intersects(bounds) {
                continue;
            }
            for e in 0..ring.segment_count() {
                let edge = ring.segment(e);
                let meet = intersect(segment, &edge);
                for point in &meet.points[..meet.count] {
                    let t = (*point - segment.start).dot(along) / length2;
                    if t > 0.0 && t < 1.0 {
                        cuts.push(t);
                    }
                }
            }
        }
    }
    // std::ranges::sort and std::unique: a cut is finite, so the order is
    // total, and equal cuts are adjacent.
    cuts.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
    cuts.dedup();
    cuts
}

/// Each sub-segment between cuts, in order, with whether it is kept.
struct Piece {
    from: Point2,
    to: Point2,
    kept: bool,
}

/// One contour cut into the pieces inside the areas.
fn clip_one(contour: &Contour, areas: &[Area<'_>], out: &mut Vec<Contour>) {
    let line = &contour.line;
    let mut pieces = Vec::new();
    for s in 0..line.segment_count() {
        let segment = line.segment(s);
        let mut at = cuts_of(&segment, areas);
        at.insert(0, 0.0);
        at.push(1.0);
        for i in 0..at.len() - 1 {
            let from = if i == 0 {
                segment.start
            } else {
                segment.point_at(at[i])
            };
            let to = if i + 2 == at.len() {
                segment.end
            } else {
                segment.point_at(at[i + 1])
            };
            if from == to {
                continue;
            }
            pieces.push(Piece {
                from,
                to,
                kept: inside(areas, (from + to) * 0.5),
            });
        }
    }
    if pieces.is_empty() {
        return;
    }
    if pieces.iter().all(|piece| piece.kept) {
        out.push(contour.clone());
        return;
    }
    // Runs of kept pieces, each an open polyline.
    let mut runs: Vec<Vec<Point2>> = Vec::new();
    let mut open = false;
    for piece in &pieces {
        if !piece.kept {
            open = false;
            continue;
        }
        if !open {
            runs.push(vec![piece.from]);
            open = true;
        }
        if let Some(run) = runs.last_mut() {
            run.push(piece.to);
        }
    }
    // A ring cut somewhere: the run through its first vertex was cut in two,
    // one at each end of the list.
    let ends_kept = pieces.first().is_some_and(|piece| piece.kept)
        && pieces.last().is_some_and(|piece| piece.kept);
    if line.closed
        && runs.len() > 1
        && ends_kept
        && let Some(mut joined) = runs.pop()
    {
        joined.extend_from_slice(&runs[0][1..]);
        runs[0] = joined;
    }
    for run in runs {
        out.push(Contour {
            elevation: contour.elevation,
            major: contour.major,
            line: Polyline2::new(run, false),
        });
    }
}

/// The contours of `set` - the lines of GDAL's `raster contour` - each at the
/// level its `elevation_field` holds. A ring GDAL closed by repeating its
/// first vertex is a closed polyline. Major when (level - base) / interval is
/// a multiple of `major_every` (0: none), as katana_terrain::contours flags
/// them. Sorted by level, GDAL's order kept within one. InvalidArgument for
/// an interval that is not positive and finite, or a table without the field
/// (the contract test pins the argument that writes it).
pub fn contours_from_features(
    set: &FeatureSet,
    elevation_field: &str,
    interval: f64,
    base: f64,
    major_every: usize,
) -> Result<Vec<Contour>> {
    if !(interval > 0.0) || !interval.is_finite() || !base.is_finite() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "a contour interval is positive and finite, and its base finite",
        ));
    }
    let mut contours = Vec::new();
    for table in &set.tables {
        if table.features.is_empty() {
            continue;
        }
        let Some(index) = table
            .fields
            .iter()
            .position(|field| field.name == elevation_field)
        else {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                format!(
                    "GDAL's contours carry no level: the table has no field '{elevation_field}'"
                ),
                table.name.clone(),
            ));
        };
        for feature in &table.features {
            let Some(level) = feature.values.get(index).and_then(number_of) else {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    format!("a contour GDAL wrote has no level in '{elevation_field}'"),
                    table.name.clone(),
                ));
            };
            // The tracer's rule (katana_terrain::contours): the k of base +
            // k * interval, major when a multiple of major_every. Rounded,
            // since GDAL computed the level in floating point too. llround
            // rounds half away from zero, as f64::round does; a quotient past
            // i64's range is undefined in the C++ and saturates here.
            let k = ((level - base) / interval).round() as i64;
            let major = major_every != 0 && k.wrapping_rem(major_every as i64) == 0;
            for part in &feature.parts {
                if part.kind != GeometryKind::LineString || part.parts.is_empty() {
                    continue;
                }
                if let Some(line) = line_of(&part.parts[0]) {
                    contours.push(Contour {
                        elevation: level,
                        major,
                        line,
                    });
                }
            }
        }
    }
    // std::ranges::stable_sort by level: equal levels keep GDAL's order.
    contours.sort_by(|a, b| {
        a.elevation
            .partial_cmp(&b.elevation)
            .unwrap_or(Ordering::Equal)
    });
    Ok(contours)
}

/// The polygons of `set` (drawing_dataset's `polygons` table: closed
/// polylines and circles, with the holes tagged to join them) as boundaries.
/// Points and open lines are not areas; the caller counts and reports them.
pub fn boundaries_of(set: &FeatureSet) -> Vec<ContourBoundary> {
    let mut boundaries = Vec::new();
    for table in &set.tables {
        for feature in &table.features {
            for part in &feature.parts {
                if part.kind != GeometryKind::Polygon {
                    continue;
                }
                let mut boundary = ContourBoundary::default();
                for ring in &part.parts {
                    if let Some(mut line) = line_of(ring)
                        && line.vertices.len() >= 3
                    {
                        line.closed = true;
                        boundary.rings.push(line);
                    }
                }
                if !boundary.rings.is_empty() {
                    boundaries.push(boundary);
                }
            }
        }
    }
    boundaries
}

/// `contours` cut where they cross a boundary, keeping what lies inside any
/// of them (on a boundary counts as inside). A ring that never leaves stays
/// a ring; one that is cut becomes the open pieces inside, the piece through
/// its first vertex joined across it. Each piece keeps its level and whether
/// it is major. Deterministic: the order of `contours`, then along each.
pub fn clip_contours(contours: &[Contour], boundaries: &[ContourBoundary]) -> Vec<Contour> {
    let areas = areas_of(boundaries);
    let mut kept = Vec::new();
    for contour in contours {
        let bounds = box_of(&contour.line);
        if areas.iter().any(|area| area.bounds.intersects(bounds)) {
            clip_one(contour, &areas, &mut kept);
        }
    }
    kept
}

/// The one command that draws `contours`: layers made where missing, then
/// the polylines, every vertex at the contour's level. It reads the model and
/// changes nothing; the caller executes it, which is how the contours become
/// one undo step. InvalidArgument for a layer that is not a layer path.
pub fn contour_command(
    model: &Model,
    contours: &[Contour],
    options: &ContourEntityOptions,
) -> Result<ContourPlan> {
    if let Err(invalid) = validate_layer_path(&options.layer) {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            format!(
                "the contours' layer is not a layer path: {}",
                invalid.message
            ),
            options.layer.clone(),
        ));
    }
    let mut plan = ContourPlan::default();
    if contours.is_empty() {
        return Ok(plan);
    }
    let major_layer = format!("{}/major", options.layer);
    let minor_layer = format!("{}/minor", options.layer);
    let mut transaction = Transaction::new(if options.command_name.is_empty() {
        "CONTOUR"
    } else {
        options.command_name.as_str()
    });
    let any_major = contours.iter().any(|contour| contour.major);
    let any_minor = !contours.iter().all(|contour| contour.major);
    for (wanted, layer) in [(any_major, &major_layer), (any_minor, &minor_layer)] {
        if !wanted {
            continue;
        }
        plan.layers.push(layer.clone());
        if !model.layers.contains(layer) {
            transaction.add(create_layer(Layer {
                name: layer.clone(),
                ..Layer::default()
            }));
        }
    }
    let mut entities = Vec::with_capacity(contours.len());
    let mut levels = Vec::with_capacity(contours.len());
    for contour in contours {
        let mut entity = Entity {
            geometry: Geometry::Polyline(contour.line.clone()),
            layer: if contour.major {
                major_layer.clone()
            } else {
                minor_layer.clone()
            },
            ..Entity::default()
        };
        set_heights(
            &mut entity.properties,
            &vec![Some(contour.elevation); contour.line.vertices.len()],
        );
        entities.push(entity);
        levels.push(contour.elevation);
        if contour.major {
            plan.major += 1;
        } else {
            plan.minor += 1;
        }
    }
    plan.created = entities.len();
    // A std::set<double>'s size: levels that compare equal (0 and -0) are
    // one.
    levels.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
    levels.dedup();
    plan.levels = levels.len();
    transaction.add(create_entities(entities));
    plan.command = Some(Box::new(transaction));
    Ok(plan)
}
