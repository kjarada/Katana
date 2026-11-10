//! Drawing data to and from the tables GDAL's algorithms read and write
//! (include/katana/interop/geo/drawing_dataset.hpp,
//! src/katana_interop/geo/drawing_dataset.cpp, docs/geoprocessing.md
//! "Bindings"): the ONE conversion between entities and features. What a
//! scope took becomes a FeatureSet an algorithm reads; the features an
//! algorithm returns become ONE undoable command on the drawing.
//!
//! Entities to features (`drawing_dataset`):
//!
//! ```text
//! three tables, `points`, `lines` and `polygons`, an empty one omitted;
//! fields: `katana_id` first (Integer64, the key a result is joined back by),
//!   then `layer`, `style`, `colour` and `type`, then the entities'
//!   properties, typed and sorted by key - a key whose type differs between
//!   entities becomes a String, and says so;
//! a closed polyline or a circle is an area (closed_as_polygons). Nested
//!   closed polylines are NOT holes - a building inside a lot is not a hole
//!   in the lot - only a ring tagged `source.ring=hole` (what IMPORT writes)
//!   or `gis.ring=hole` (what a result writes) joins the exterior it lies in,
//!   the one with its `gis.part` (`source.part`, IMPORT's) when it has one;
//! arcs and circles are chords within curve_tolerance (the export's 1 mm
//!   sagitta by default); so are the draw system's curves: a curve polyline
//!   (an area when closed, its heights its vertices' own and a chord point's
//!   linear by length along its segment), an ellipse (an area when whole)
//!   and a spline (an area when closed), those two in plan;
//! Z only when every vertex has a height (absent is not zero); with
//!   require_heights a heightless entity is left out and counted;
//! text, dimensions, labels and leaders have no feature; they are left out
//!   and counted by kind;
//! every table carries the project's coordinate system.
//! ```
//!
//! Features to the drawing (`result_command`): one commands::Transaction.
//!
//! ```text
//! Create          entities on the target layer (a table of several goes to
//!                 <layer>/<table>), layers made where missing; typed fields
//!                 become typed properties, with gis.op=<algorithm> and
//!                 gis.source=<katana_id> when a feature carries one; a
//!                 polygon's rings are closed polylines tagged gis.ring and
//!                 gis.part. No area or length is written: a measure kept on
//!                 an entity goes stale when it is edited, so replies carry
//!                 measures instead.
//! UpdateGeometry  the geometry of the entity each katana_id names; its id,
//!                 and so its labels and associations, are kept.
//! SetProperties   the fields as properties of the entity each katana_id
//!                 names.
//! ```
//!
//! A REPLACE is the verb's own: it deletes what its scope took (GIS
//! DISSOLVE) or what fell outside (GIS CLIP), the holes an area took in
//! (DrawingDataset::holes) with it, in the same step as this command.

use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet};

use katana_commands::{
    CommandPtr, Transaction, create_entities, create_layer, remove_entity_property,
    set_entity_geometry, set_entity_property,
};
use katana_core::error::make_error;
use katana_core::text::{format_exact_real, is_ascii_space};
use katana_core::{ErrorCode, Result};
use katana_entity::entity::{
    ELEVATION_PROPERTY, ELEVATIONS_PROPERTY, heights_of, property_to_string, set_heights, to_string,
};
use katana_entity::layer_path::{LAYER_SEPARATOR, validate_layer_path};
use katana_entity::ring_tags::{is_tagged_hole, ring_part};
use katana_entity::{
    Entity, EntityId, ExteriorCandidate, Geometry, HoleJoiner, Layer, Model, PointGeometry,
    PropertyMap, PropertyValue,
};
use katana_geometry::{Arc2, Circle2, Point2, Polyline2, Segment2, Vec2};
use katana_gis::processing::{Feature, FeatureSet, FeatureTable, FieldDef, FieldType, FieldValue};
use katana_gis::{GeoPoint, GeometryKind, VectorGeometry};
use katana_math::numerics::TWO_PI;

use crate::curve_chords::chord_count;

#[derive(Clone, Debug, PartialEq)]
pub struct DrawingDatasetOptions {
    /// The largest distance a chord may stray from an arc or a circle, in
    /// model units.
    pub curve_tolerance: f64,
    /// Leave out, and count as "heightless", an entity without a height at
    /// every vertex: what an algorithm that reads Z (gridding, rasterize
    /// --3d) must be given, since a missing height read as 0 is a wrong
    /// surface.
    pub require_heights: bool,
    pub closed_as_polygons: bool,
    /// The project's coordinate system; empty for none.
    pub crs_wkt: String,
    /// Every feature in ONE table, `table_name`, whatever its kind: a file's
    /// layer that holds points, lines and areas together (EXPORT). Its kind
    /// is theirs when they share one, else Unknown.
    pub one_table: bool,
    pub table_name: String,
    /// `style`, `colour` and `type` after `katana_id` and `layer`: what an
    /// algorithm's result is filtered and styled by. EXPORT leaves them out -
    /// a file handed to someone else carries each entity's id and layer, as
    /// it always has, and the drawing's styling stays the drawing's.
    pub style_fields: bool,
    /// The entities' properties as fields, after those.
    pub properties: bool,
}

impl Default for DrawingDatasetOptions {
    fn default() -> Self {
        Self {
            curve_tolerance: 0.001,
            require_heights: false,
            closed_as_polygons: true,
            crs_wkt: String::new(),
            one_table: false,
            table_name: String::from("features"),
            style_fields: true,
            properties: true,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DrawingDatasetStats {
    /// The ids asked for.
    pub matched: usize,
    /// Entities that went into a feature.
    pub used: usize,
    /// Features written.
    pub points: usize,
    pub lines: usize,
    pub polygons: usize,
    /// Reason -> count.
    pub skipped: BTreeMap<String, usize>,
    pub warnings: Vec<String>,
    /// Entities IMPORT moved from their file's coordinates (SHIFT_KEY): when
    /// any is used, no table claims the project's coordinate system.
    pub shifted: usize,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DrawingDataset {
    pub set: FeatureSet,
    pub stats: DrawingDatasetStats,
    /// The hole entities each area took in, by the area's id, in the order
    /// they were joined. A feature carries only its area's katana_id, so an
    /// in-place verb that deletes an area (GIS CLIP REPLACE) finds its holes
    /// here: left behind, a hole would be read as an area of its own.
    pub holes: BTreeMap<EntityId, Vec<EntityId>>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ResultMode {
    #[default]
    Create,
    UpdateGeometry,
    SetProperties,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ResultOptions {
    /// Create: where the entities go. Several tables go to <layer>/<table>.
    pub target_layer: String,
    pub mode: ResultMode,
    /// gis.op on what is created: the algorithm, "raster contour".
    pub operation: String,
    /// Put before each field's name when it becomes a property ("zone." ->
    /// zone.mean); empty for none.
    pub property_prefix: String,
    /// The transaction's name: the verb line, so the history says what ran.
    pub command_name: String,
}

#[derive(Default)]
pub struct ResultPlan {
    /// None when there is nothing to do: an empty transaction is refused by
    /// the command stack, and a result of nothing changes nothing.
    pub command: Option<CommandPtr>,
    pub created: usize,
    pub updated: usize,
    /// Stays 0: result_command deletes nothing (a verb's REPLACE counts its
    /// own); the GDAL verb's output record still says it.
    pub deleted: usize,
    pub skipped: usize,
    /// The layers written to, in first-use order.
    pub layers: Vec<String>,
    /// What could not be carried, said.
    pub warnings: Vec<String>,
}

/// The field names drawing_dataset writes before the properties, which a
/// result does not turn back into properties: katana_id, layer, style,
/// colour, type.
const BOOKKEEPING: [&str; 5] = ["katana_id", "layer", "style", "colour", "type"];

/// A property holds a date as its ISO 8601 text, so a Date or DateTime field
/// is kept as the entity's metadata `<prefix><key>` = "date" | "datetime":
/// IMPORT writes IMPORT_TYPE_PREFIX, a result RESULT_TYPE_PREFIX, and
/// drawing_dataset gives a text property so tagged on every entity that
/// holds it the field type again. Without it a date came back from EXPORT a
/// String.
pub const IMPORT_TYPE_PREFIX: &str = "source.type.";
/// The metadata IMPORT writes on an entity it moved from its file's
/// coordinates (LOCAL, ALONGSIDE, OFFSET=): "dE,dN", the shift subtracted.
/// Such an entity is in neither the file's coordinate system nor the
/// project's, so drawing_dataset gives no table with one in it a coordinate
/// system, and says so: a file EXPORT wrote with the project's labelled
/// shifted coordinates as georeferenced ones.
pub const SHIFT_KEY: &str = "source.shift";
pub const RESULT_TYPE_PREFIX: &str = "gis.type.";

/// One feature geometry as the entity geometries it becomes: the ONE way a
/// feature becomes entities, for a result (result_command) and for IMPORT
/// alike (docs/interop.md, "Conversion").
///
/// ```text
/// a point is a point;
/// a line of two points is a Line, a longer one a polyline, closed when it
///   ends where it began at the same height (a DXF circle and a closed
///   LWPOLYLINE arrive so);
/// a curve that is one arc is an Arc, and one that is a whole circle a
///   Circle; any other curve is chords within `curve_tolerance`, by the rule
///   EXPORT makes its chords by, and counted;
/// an area is its rings, each a closed polyline, `ring` saying which is the
///   exterior and which a hole (a circle alone is a Circle).
/// ```
///
/// Consecutive repeated points are dropped (a zero-length segment is one the
/// model refuses), and a height that goes with one is counted as lost.
#[derive(Clone, Debug, PartialEq)]
pub struct FeaturePiece {
    pub geometry: Geometry,
    /// One per vertex; an arc's two ends; a circle's one.
    pub heights: Vec<Option<f64>>,
    /// "exterior" or "hole", for an area's rings.
    pub ring: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FeaturePieceOptions {
    /// Subtracted from every coordinate: IMPORT's local origin.
    pub origin_shift: Option<Vec2>,
    pub curve_tolerance: f64,
}

impl Default for FeaturePieceOptions {
    fn default() -> Self {
        Self {
            origin_shift: None,
            curve_tolerance: 0.001,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FeaturePieceCounts {
    /// Geometries with too few distinct points to draw.
    pub degenerate: usize,
    /// Points dropped on the one before them in plan, at another height.
    pub heights_lost: usize,
    pub curves_made_chords: usize,
}

// ---- entities to features ----------------------------------------------------------------

/// One entity's geometry as a ring or a string of points, before it is put
/// in a table.
struct Shape<'a> {
    entity: &'a Entity,
    kind: GeometryKind,
    /// A polygon's exterior ring, not repeated at the end.
    points: Vec<GeoPoint>,
    has_z: bool,
    holes: Vec<Vec<GeoPoint>>,
    holes_have_z: bool,
    is_hole: bool,
}

fn plan_points(points: &[Point2]) -> Vec<GeoPoint> {
    points
        .iter()
        .map(|point| GeoPoint {
            x: point.x,
            y: point.y,
            z: 0.0,
        })
        .collect()
}

fn arc_points(arc: &Arc2, tolerance: f64) -> Vec<Point2> {
    let chords = chord_count(arc.radius, arc.sweep, tolerance);
    (0..=chords)
        .map(|i| arc.point_at(f64::from(i) / f64::from(chords)))
        .collect()
}

fn circle_points(circle: &Circle2, tolerance: f64) -> Vec<Point2> {
    let chords = chord_count(circle.radius, TWO_PI, tolerance).max(3);
    (0..chords)
        .map(|i| circle.point_at_angle(TWO_PI * f64::from(i) / f64::from(chords)))
        .collect()
}

/// Heights onto the points when every one has one: a geometry is 3D only
/// then.
fn apply_heights(points: &mut [GeoPoint], heights: &[Option<f64>]) -> bool {
    if points.is_empty() || heights.len() != points.len() || heights.iter().any(Option::is_none) {
        return false;
    }
    for (point, z) in points.iter_mut().zip(heights) {
        point.z = z.unwrap_or_default();
    }
    true
}

/// Entities that went in plan although they have heights, and why: said,
/// since their heights then travel only as the elevation fields.
#[derive(Default)]
struct HeightNotes {
    /// Heights at only some vertices.
    partial: usize,
    /// An arc whose two ends differ.
    sloping_arcs: usize,
}

/// The shape of `entity`, or the reason it has none.
fn shape_of<'a>(
    entity: &'a Entity,
    options: &DrawingDatasetOptions,
    notes: &mut HeightNotes,
) -> std::result::Result<Shape<'a>, &'static str> {
    let props = &entity.properties;
    let (kind, mut points, heights) = match &entity.geometry {
        Geometry::Point(held) => (
            GeometryKind::Point,
            plan_points(&[held.position]),
            heights_of(props, 1),
        ),
        Geometry::Line(held) => (
            GeometryKind::LineString,
            plan_points(&[held.start, held.end]),
            heights_of(props, 2),
        ),
        Geometry::Arc(held) => {
            let points = plan_points(&arc_points(held, options.curve_tolerance));
            // An arc carries a height at each end and none between: level,
            // every chord point has it; sloping, the heights between would
            // be invented, so it goes in plan - EXPORT's rule.
            let ends = heights_of(props, 2);
            let mut heights = Vec::new();
            if ends[0].is_some() && ends[1].is_some() && ends[0] == ends[1] {
                heights = vec![ends[0]; points.len()];
            } else if ends[0].is_some() || ends[1].is_some() {
                notes.sloping_arcs += 1;
            }
            (GeometryKind::LineString, points, heights)
        }
        Geometry::Polyline(held) => {
            let mut points = plan_points(&held.vertices);
            let mut heights = heights_of(props, held.vertices.len());
            if held.closed && held.vertices.len() >= 3 && options.closed_as_polygons {
                (GeometryKind::Polygon, points, heights)
            } else {
                if held.closed && !points.is_empty() {
                    points.push(points[0]);
                    if !heights.is_empty() {
                        heights.push(heights[0]);
                    }
                }
                (GeometryKind::LineString, points, heights)
            }
        }
        Geometry::Circle(held) => {
            let mut points = plan_points(&circle_points(held, options.curve_tolerance));
            let mut heights = vec![heights_of(props, 1)[0]; points.len()];
            if options.closed_as_polygons {
                (GeometryKind::Polygon, points, heights)
            } else {
                points.push(points[0]);
                heights.push(heights[0]);
                (GeometryKind::LineString, points, heights)
            }
        }
        Geometry::CurvePolyline(held) => {
            // Chords within the tolerance, each chord point at the height the
            // geometry's own rule gives it (linear by length along its
            // segment; none where an end has none) - EXPORT's rule. The
            // heights are the geometry's, not the elevation properties: a
            // curve polyline holds them itself (docs/drawing.md).
            let walk = held.tessellate_with_heights(options.curve_tolerance);
            let positions: Vec<Point2> = walk.iter().map(|step| step.position).collect();
            let mut heights: Vec<Option<f64>> = walk.iter().map(|step| step.height).collect();
            let mut points = plan_points(&positions);
            // The walk repeats the first point when closed; an area's ring
            // does not.
            if held.closed && points.len() >= 4 && options.closed_as_polygons {
                points.pop();
                heights.pop();
                (GeometryKind::Polygon, points, heights)
            } else {
                (GeometryKind::LineString, points, heights)
            }
        }
        // Chords within the tolerance, in plan: neither holds a height, and
        // a height taken from an elevation property would be one the drawing
        // never gave the curve between.
        Geometry::Ellipse(held) => {
            let mut points = plan_points(&held.tessellate(options.curve_tolerance));
            let kind = closed_curve_kind(&mut points, held.is_full(), options);
            (kind, points, Vec::new())
        }
        Geometry::Spline(held) => {
            let mut points = plan_points(&held.tessellate(options.curve_tolerance));
            let kind = closed_curve_kind(&mut points, held.is_closed_shape(), options);
            (kind, points, Vec::new())
        }
        Geometry::Text(_) => return Err("text"),
        Geometry::Dimension(_) => return Err("dimension"),
        Geometry::Label(_) => return Err("label"),
        Geometry::Leader(_) => return Err("leader"),
    };
    let least = match kind {
        GeometryKind::Point => 1,
        GeometryKind::LineString => 2,
        _ => 3,
    };
    if points.len() < least {
        return Err("degenerate");
    }
    let has_z = apply_heights(&mut points, &heights);
    if options.require_heights && !has_z {
        return Err("heightless");
    }
    if !has_z && heights.iter().any(Option::is_some) {
        notes.partial += 1;
    }
    Ok(Shape {
        entity,
        kind,
        points,
        has_z,
        holes: Vec::new(),
        holes_have_z: true,
        is_hole: kind == GeometryKind::Polygon && is_tagged_hole(entity),
    })
}

/// An ellipse's or a spline's chords: an area when the curve closes on
/// itself (its last chord point repeating its first, dropped), else a line.
fn closed_curve_kind(
    points: &mut Vec<GeoPoint>,
    shut: bool,
    options: &DrawingDatasetOptions,
) -> GeometryKind {
    if shut && points.len() >= 4 && options.closed_as_polygons {
        points.pop();
        GeometryKind::Polygon
    } else {
        GeometryKind::LineString
    }
}

/// The shapes of one kind, in order; a hole taken into its area is of none.
fn of_kind<'s, 'a>(shapes: &'s [Shape<'a>], kind: GeometryKind) -> Vec<&'s Shape<'a>> {
    shapes.iter().filter(|shape| shape.kind == kind).collect()
}

fn ring_of(points: &[GeoPoint]) -> Polyline2 {
    Polyline2 {
        vertices: points
            .iter()
            .map(|point| Point2::new(point.x, point.y))
            .collect(),
        closed: true,
    }
}

/// Tagged holes join the area they lie in, by the one rule of
/// katana_entity's ring_tags (the volume verbs read a lot with a hole the
/// same way). A hole with no area around it stays an area of its own, and
/// says so.
#[allow(
    clippy::needless_range_loop,
    reason = "A hole and the area it joins are two elements of one slice, the area changed while the hole is read."
)]
fn join_holes(
    shapes: &mut [Shape<'_>],
    stats: &mut DrawingDatasetStats,
    joined: &mut BTreeMap<EntityId, Vec<EntityId>>,
) {
    let mut areas = Vec::new();
    let mut exteriors = Vec::new();
    for (i, shape) in shapes.iter().enumerate() {
        if shape.kind == GeometryKind::Polygon && !shape.is_hole {
            areas.push(i);
            exteriors.push(ExteriorCandidate {
                id: shape.entity.id,
                part: ring_part(shape.entity),
                ring: ring_of(&shape.points),
            });
        }
    }
    let joiner = HoleJoiner::new(exteriors);
    let mut lost = 0;
    for h in 0..shapes.len() {
        if !shapes[h].is_hole {
            continue;
        }
        let hole_id = shapes[h].entity.id;
        let corners: Vec<Point2> = shapes[h]
            .points
            .iter()
            .map(|point| Point2::new(point.x, point.y))
            .collect();
        let Some(best) = joiner.exterior_of(&corners, hole_id, &ring_part(shapes[h].entity)) else {
            shapes[h].is_hole = false;
            lost += 1;
            continue;
        };
        let hole_points = shapes[h].points.clone();
        let hole_has_z = shapes[h].has_z;
        let area = &mut shapes[areas[best]];
        area.holes.push(hole_points);
        area.holes_have_z = area.holes_have_z && hole_has_z;
        joined.entry(area.entity.id).or_default().push(hole_id);
        // Taken into its area.
        shapes[h].kind = GeometryKind::Unknown;
        stats.used += 1;
    }
    if lost != 0 {
        stats.warnings.push(format!(
            "{lost} rings tagged as holes lie in no area in scope and were kept as areas of \
             their own"
        ));
    }
}

fn field_type_of(value: &PropertyValue) -> FieldType {
    match value {
        PropertyValue::Boolean(_) => FieldType::Boolean,
        PropertyValue::Integer(_) => FieldType::Integer64,
        PropertyValue::Real(_) => FieldType::Real,
        PropertyValue::Text(_) => FieldType::String,
    }
}

/// A text property's type as its entity's metadata keeps it (a date), else
/// text.
fn text_type_of(entity: &Entity, key: &str) -> FieldType {
    for prefix in [IMPORT_TYPE_PREFIX, RESULT_TYPE_PREFIX] {
        let Some(tag) = entity.metadata.get(&format!("{prefix}{key}")) else {
            continue;
        };
        let text = property_to_string(tag);
        if text == "date" {
            return FieldType::Date;
        }
        if text == "datetime" {
            return FieldType::DateTime;
        }
    }
    FieldType::String
}

fn field_value_of(value: &PropertyValue, r#type: FieldType) -> FieldValue {
    if r#type == FieldType::String || r#type == FieldType::Date || r#type == FieldType::DateTime {
        return FieldValue::Text(property_to_string(value));
    }
    match value {
        PropertyValue::Boolean(held) => FieldValue::Boolean(*held),
        PropertyValue::Integer(held) => FieldValue::Integer64(*held),
        PropertyValue::Real(held) => FieldValue::Real(*held),
        PropertyValue::Text(held) => FieldValue::Text(held.clone()),
    }
}

fn lower_type(entity: &Entity) -> String {
    to_string(entity.r#type()).to_ascii_lowercase()
}

fn table_of(
    name: &str,
    kind: GeometryKind,
    shapes: &[&Shape<'_>],
    options: &DrawingDatasetOptions,
    stats: &mut DrawingDatasetStats,
) -> FeatureTable {
    // The bookkeeping written: katana_id and layer always, style, colour and
    // type unless the caller leaves them out.
    let leading = if options.style_fields {
        BOOKKEEPING.len()
    } else {
        2
    };
    // The properties' fields: typed when every entity agrees, text when not.
    let mut types: BTreeMap<String, FieldType> = BTreeMap::new();
    let mut mixed: BTreeSet<String> = BTreeSet::new();
    let mut dropped = 0;
    if options.properties {
        for shape in shapes {
            for (key, value) in &shape.entity.properties {
                if BOOKKEEPING[..leading]
                    .iter()
                    .any(|field| key.eq_ignore_ascii_case(field))
                {
                    dropped += 1;
                    continue;
                }
                let mut r#type = field_type_of(value);
                if r#type == FieldType::String {
                    r#type = text_type_of(shape.entity, key);
                }
                match types.entry(key.clone()) {
                    Entry::Vacant(slot) => {
                        slot.insert(r#type);
                    }
                    Entry::Occupied(mut slot) => {
                        if *slot.get() != r#type {
                            *slot.get_mut() = FieldType::String;
                            mixed.insert(key.clone());
                        }
                    }
                }
            }
        }
    }
    for key in &mixed {
        stats.warnings.push(format!(
            "property {key} holds values of more than one type in {name}; it is text there"
        ));
    }
    if dropped != 0 {
        let mut fields = String::new();
        for (f, field) in BOOKKEEPING[..leading].iter().enumerate() {
            fields.push_str(if f == 0 {
                ""
            } else if f + 1 == leading {
                " or "
            } else {
                ", "
            });
            fields.push_str(field);
        }
        stats.warnings.push(format!(
            "{dropped} property values named like the fields {fields} were left out of {name}"
        ));
    }
    let mut table = FeatureTable {
        name: String::from(name),
        kind,
        crs_wkt: options.crs_wkt.clone(),
        fields: vec![
            FieldDef {
                name: String::from("katana_id"),
                r#type: FieldType::Integer64,
            },
            FieldDef {
                name: String::from("layer"),
                r#type: FieldType::String,
            },
        ],
        ..FeatureTable::default()
    };
    if options.style_fields {
        for field in ["style", "colour", "type"] {
            table.fields.push(FieldDef {
                name: String::from(field),
                r#type: FieldType::String,
            });
        }
    }
    for (key, r#type) in &types {
        table.fields.push(FieldDef {
            name: key.clone(),
            r#type: *r#type,
        });
    }
    for shape in shapes {
        let entity = shape.entity;
        let mut parts = vec![shape.points.clone()];
        parts.extend(shape.holes.iter().cloned());
        let geometry = VectorGeometry {
            kind: shape.kind,
            parts,
            has_z: shape.has_z && (shape.holes.is_empty() || shape.holes_have_z),
            arcs: Vec::new(),
        };
        table.has_z = table.has_z || geometry.has_z;
        let mut values = vec![
            FieldValue::Integer64(entity.id as i64),
            FieldValue::Text(entity.layer.clone()),
        ];
        if options.style_fields {
            values.push(FieldValue::Text(if entity.style.is_empty() {
                String::from("ByLayer")
            } else {
                entity.style.clone()
            }));
            values.push(FieldValue::Text(
                entity
                    .color
                    .map_or_else(|| String::from("ByLayer"), |color| color.to_hex()),
            ));
            values.push(FieldValue::Text(lower_type(entity)));
        }
        for (key, r#type) in &types {
            values.push(
                entity
                    .properties
                    .get(key)
                    .map_or(FieldValue::Null, |found| field_value_of(found, *r#type)),
            );
        }
        table.features.push(Feature {
            parts: vec![geometry],
            values,
        });
    }
    table
}

// ---- features to the drawing ------------------------------------------------------------

fn property_of(value: &FieldValue) -> Option<PropertyValue> {
    match value {
        FieldValue::Null => None,
        FieldValue::Boolean(held) => Some(PropertyValue::Boolean(*held)),
        FieldValue::Integer64(held) => Some(PropertyValue::Integer(*held)),
        FieldValue::Real(held) => Some(PropertyValue::Real(*held)),
        FieldValue::Text(held) => Some(PropertyValue::Text(held.clone())),
    }
}

fn source_of(table: &FeatureTable, feature: &Feature) -> Option<EntityId> {
    for (field, value) in table.fields.iter().zip(&feature.values) {
        if field.name != "katana_id" {
            continue;
        }
        if let FieldValue::Integer64(id) = value
            && *id > 0
        {
            return Some(*id as EntityId);
        }
        // The C++'s static_cast of a double past EntityId's range is
        // undefined; Rust's cast saturates.
        if let FieldValue::Real(real) = value
            && *real >= 1.0
            && real.is_finite()
        {
            return Some(*real as EntityId);
        }
    }
    None
}

fn has_katana_id(table: &FeatureTable) -> bool {
    table.fields.iter().any(|field| field.name == "katana_id")
}

/// A vertex on its way to an entity: where it is, and its height if it has
/// one.
struct Vertex {
    at: Point2,
    z: Option<f64>,
}

fn shifted(options: &FeaturePieceOptions, raw: &GeoPoint) -> Point2 {
    match options.origin_shift {
        Some(shift) => Point2::new(raw.x - shift.x, raw.y - shift.y),
        None => Point2::new(raw.x, raw.y),
    }
}

fn height_of(has_z: bool, raw: &GeoPoint) -> Option<f64> {
    (has_z && raw.z.is_finite()).then_some(raw.z)
}

/// A part's points as the vertices of the line it becomes, its arcs made
/// chords by the rule EXPORT makes them by (curve_chords.rs). A level arc's
/// chords carry its height; a sloping one's inner chord points carry none,
/// since a height between an arc's ends would be invented.
fn vertices_of(
    source: &[GeoPoint],
    arcs: Option<&Vec<usize>>,
    has_z: bool,
    options: &FeaturePieceOptions,
    counts: &mut FeaturePieceCounts,
) -> Vec<Vertex> {
    let mut out = Vec::with_capacity(source.len());
    let mut i = 0;
    while i < source.len() {
        let arc_here = arcs.is_some_and(|arcs| arcs.contains(&i)) && i + 2 < source.len();
        if !arc_here {
            out.push(Vertex {
                at: shifted(options, &source[i]),
                z: height_of(has_z, &source[i]),
            });
            i += 1;
            continue;
        }
        let start = shifted(options, &source[i]);
        let end = shifted(options, &source[i + 2]);
        let arc = Arc2::through_points(start, shifted(options, &source[i + 1]), end);
        let z0 = height_of(has_z, &source[i]);
        let z1 = height_of(has_z, &source[i + 2]);
        out.push(Vertex { at: start, z: z0 });
        if let Some(arc) = arc {
            counts.curves_made_chords += 1;
            let chords = chord_count(arc.radius, arc.sweep, options.curve_tolerance);
            let inner = if z0.is_some() && z1.is_some() && z0 == z1 {
                z0
            } else {
                None
            };
            for k in 1..chords {
                out.push(Vertex {
                    at: arc.point_at(f64::from(k) / f64::from(chords)),
                    z: inner,
                });
            }
        }
        // The end is the next arc's start or the next straight run's first
        // point, taken as the loop reaches it.
        i += 2;
    }
    out
}

/// Consecutive repeats dropped, as IMPORT drops them: a zero-length segment
/// is one the model refuses. A repeat at another height takes that height
/// with it.
struct Vertices {
    points: Vec<Point2>,
    heights: Vec<Option<f64>>,
}

fn distinct(source: &[Vertex], counts: &mut FeaturePieceCounts) -> Vertices {
    let mut out = Vertices {
        points: Vec::with_capacity(source.len()),
        heights: Vec::with_capacity(source.len()),
    };
    for vertex in source {
        if !vertex.at.x.is_finite() || !vertex.at.y.is_finite() {
            continue;
        }
        if let (Some(last), Some(height)) = (out.points.last(), out.heights.last())
            && *last == vertex.at
        {
            counts.heights_lost += usize::from(vertex.z != *height);
            continue;
        }
        out.points.push(vertex.at);
        out.heights.push(vertex.z);
    }
    out
}

fn arcs_of_part(geometry: &VectorGeometry, part: usize) -> Option<&Vec<usize>> {
    geometry.arcs.get(part).filter(|arcs| !arcs.is_empty())
}

/// A part that is one arc and nothing else: its start, middle and end.
fn is_one_arc(geometry: &VectorGeometry, part: usize) -> bool {
    arcs_of_part(geometry, part).is_some_and(|arcs| arcs.len() == 1 && arcs[0] == 0)
        && geometry
            .parts
            .get(part)
            .is_some_and(|points| points.len() == 3)
}

/// The circle a CircularString of three points is when it ends where it
/// began: its middle point is the far side, so the centre is halfway to it.
fn whole_circle(start: Point2, far: Point2) -> Option<Circle2> {
    let radius = (far.x - start.x).hypot(far.y - start.y) / 2.0;
    if !(radius > 0.0) || !radius.is_finite() {
        return None;
    }
    Some(Circle2::new(
        Point2::new((start.x + far.x) / 2.0, (start.y + far.y) / 2.0),
        radius,
    ))
}

/// One arc part as an Arc or a Circle entity; None when its points are in a
/// line, which then goes as the straight line it is.
fn arc_piece(
    geometry: &VectorGeometry,
    part: usize,
    options: &FeaturePieceOptions,
) -> Option<FeaturePiece> {
    let points = &geometry.parts[part];
    let z = |raw: &GeoPoint| height_of(geometry.has_z, raw);
    let start = shifted(options, &points[0]);
    let end = shifted(options, &points[2]);
    if start == end {
        return whole_circle(start, shifted(options, &points[1])).map(|circle| FeaturePiece {
            geometry: Geometry::Circle(circle),
            heights: vec![z(&points[0])],
            ring: None,
        });
    }
    Arc2::through_points(start, shifted(options, &points[1]), end).map(|arc| FeaturePiece {
        geometry: Geometry::Arc(arc),
        heights: vec![z(&points[0]), z(&points[2])],
        ring: None,
    })
}

fn vertices_of_part(
    geometry: &VectorGeometry,
    part: usize,
    options: &FeaturePieceOptions,
    counts: &mut FeaturePieceCounts,
) -> Vec<Vertex> {
    let source = geometry.parts.get(part).map_or(&[][..], Vec::as_slice);
    vertices_of(
        source,
        arcs_of_part(geometry, part),
        geometry.has_z,
        options,
        counts,
    )
}

fn pieces_of(
    geometry: &VectorGeometry,
    options: &FeaturePieceOptions,
    counts: &mut FeaturePieceCounts,
) -> Vec<FeaturePiece> {
    let mut pieces = Vec::new();
    match geometry.kind {
        GeometryKind::Point => {
            let vertices = distinct(&vertices_of_part(geometry, 0, options, counts), counts);
            match (vertices.points.first(), vertices.heights.first()) {
                (Some(&position), Some(&height)) => pieces.push(FeaturePiece {
                    geometry: Geometry::Point(PointGeometry { position }),
                    heights: vec![height],
                    ring: None,
                }),
                _ => counts.degenerate += 1,
            }
        }
        GeometryKind::LineString => {
            if is_one_arc(geometry, 0)
                && let Some(piece) = arc_piece(geometry, 0, options)
            {
                pieces.push(piece);
                return pieces;
            }
            let mut vertices = distinct(&vertices_of_part(geometry, 0, options, counts), counts);
            // A line that ends where it began is a closed one. GDAL reads a
            // DXF CIRCLE, and a closed LWPOLYLINE, as a line string that
            // repeats its first point, and taken as it came a circle was an
            // OPEN polyline with a seam: no area, no hatch, and Explode or
            // Offset treating it as a path with two ends. As a polygon ring
            // does, the repeat becomes the `closed` flag. Four points at
            // least: three with the ends equal are a line there and back,
            // which encloses nothing. The ends must meet in height too (both
            // absent, or equal): a ramp or helix that comes back over its
            // start a level higher is an open string, and closing it would
            // throw away its top height.
            let mut closed = false;
            if vertices.points.len() >= 4
                && vertices.points.first() == vertices.points.last()
                && vertices.heights.first() == vertices.heights.last()
            {
                vertices.points.pop();
                vertices.heights.pop();
                closed = true;
            }
            if vertices.points.len() < 2 {
                counts.degenerate += 1;
                return pieces;
            }
            // Two points is a line in every CAD program; anything longer is
            // a polyline. A two-point polyline would be unfilletable and odd
            // to edit.
            if vertices.points.len() == 2 && !closed {
                pieces.push(FeaturePiece {
                    geometry: Geometry::Line(Segment2::new(vertices.points[0], vertices.points[1])),
                    heights: vertices.heights,
                    ring: None,
                });
            } else {
                pieces.push(FeaturePiece {
                    geometry: Geometry::Polyline(Polyline2::new(vertices.points, closed)),
                    heights: vertices.heights,
                    ring: None,
                });
            }
        }
        GeometryKind::Polygon => {
            // A circle, whole, and nothing cut out of it: the Circle it was.
            if geometry.parts.len() == 1
                && is_one_arc(geometry, 0)
                && let Some(mut piece) = arc_piece(geometry, 0, options)
                && matches!(piece.geometry, Geometry::Circle(_))
            {
                piece.ring = Some(String::from("exterior"));
                pieces.push(piece);
                return pieces;
            }
            for r in 0..geometry.parts.len() {
                let mut vertices =
                    distinct(&vertices_of_part(geometry, r, options, counts), counts);
                // A closed ring repeats its first point; the Polyline2
                // `closed` flag expresses that instead.
                if vertices.points.len() > 1 && vertices.points.first() == vertices.points.last() {
                    vertices.points.pop();
                    counts.heights_lost +=
                        usize::from(vertices.heights.last() != vertices.heights.first());
                    vertices.heights.pop();
                }
                if vertices.points.len() < 3 {
                    if r == 0 {
                        counts.degenerate += 1;
                        return Vec::new();
                    }
                    continue;
                }
                pieces.push(FeaturePiece {
                    geometry: Geometry::Polyline(Polyline2::new(vertices.points, true)),
                    heights: vertices.heights,
                    ring: Some(String::from(if r == 0 { "exterior" } else { "hole" })),
                });
            }
        }
        GeometryKind::Unknown => counts.degenerate += 1,
    }
    pieces
}

fn sanitized_table_name(name: &str) -> String {
    let replaced: String = name
        .chars()
        .map(|c| if c == LAYER_SEPARATOR { '_' } else { c })
        .collect();
    // katana_core::text::trimmed: the C locale's white space, at the ends.
    let out = replaced.trim_matches(|c: char| c.is_ascii() && is_ascii_space(c as u8));
    if out.is_empty() || out == "." || out == ".." {
        String::from("features")
    } else {
        String::from(out)
    }
}

fn transaction_named(options: &ResultOptions) -> Transaction {
    Transaction::new(if options.command_name.is_empty() {
        "GDAL"
    } else {
        options.command_name.as_str()
    })
}

fn create_plan(model: &Model, set: &FeatureSet, options: &ResultOptions) -> Result<ResultPlan> {
    if let Err(invalid) = validate_layer_path(&options.target_layer) {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            format!("the target is not a layer path: {}", invalid.message),
            options.target_layer.clone(),
        ));
    }
    let mut plan = ResultPlan::default();
    let mut transaction = transaction_named(options);
    let tables_with_features = set
        .tables
        .iter()
        .filter(|table| !table.features.is_empty())
        .count();
    let mut entities: Vec<Entity> = Vec::new();
    // The id the first entity will be given (ids are monotonic and the
    // transaction creates them in order), so a polygon's rings name their
    // exterior in gis.part. A later change that broke the prediction would
    // cost only the hint: drawing_dataset checks that a hole lies inside the
    // area its part names before it joins them.
    let first_id = model.entities.next_id();
    let mut counts = FeaturePieceCounts::default();
    for table in set.tables.iter().filter(|table| !table.features.is_empty()) {
        let layer = if tables_with_features > 1 {
            format!(
                "{}/{}",
                options.target_layer,
                sanitized_table_name(&table.name)
            )
        } else {
            options.target_layer.clone()
        };
        if let Err(invalid) = validate_layer_path(&layer) {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                format!(
                    "a result table's layer is not a layer path: {}",
                    invalid.message
                ),
                layer,
            ));
        }
        if !plan.layers.contains(&layer) {
            plan.layers.push(layer.clone());
            if !model.layers.contains(&layer) {
                transaction.add(create_layer(Layer {
                    name: layer.clone(),
                    ..Layer::default()
                }));
            }
        }
        for feature in &table.features {
            let mut properties = PropertyMap::new();
            for (field, value) in table.fields.iter().zip(&feature.values) {
                if is_bookkeeping_field(&field.name) {
                    continue;
                }
                if let Some(value) = property_of(value) {
                    properties.insert(format!("{}{}", options.property_prefix, field.name), value);
                }
            }
            if !options.operation.is_empty() {
                properties.insert(
                    String::from("gis.op"),
                    PropertyValue::Text(options.operation.clone()),
                );
            }
            if let Some(source) = source_of(table, feature) {
                properties.insert(
                    String::from("gis.source"),
                    PropertyValue::Integer(source as i64),
                );
            }
            for part in &feature.parts {
                let exterior = entities.len();
                // The rings are tagged only where there are holes to join.
                let with_holes = part.kind == GeometryKind::Polygon && part.parts.len() > 1;
                for piece in pieces_of(part, &FeaturePieceOptions::default(), &mut counts) {
                    let mut entity = Entity {
                        geometry: piece.geometry,
                        layer: layer.clone(),
                        properties: properties.clone(),
                        ..Entity::default()
                    };
                    for field in &table.fields {
                        let key = format!("{}{}", options.property_prefix, field.name);
                        if let Some(tag) = date_type_tag(field.r#type)
                            && entity.properties.contains_key(&key)
                        {
                            entity.metadata.insert(
                                format!("{RESULT_TYPE_PREFIX}{key}"),
                                PropertyValue::Text(tag),
                            );
                        }
                    }
                    if with_holes && let Some(ring) = piece.ring {
                        entity
                            .properties
                            .insert(String::from("gis.ring"), PropertyValue::Text(ring));
                        entity.properties.insert(
                            String::from("gis.part"),
                            PropertyValue::Integer((first_id + exterior as EntityId) as i64),
                        );
                    }
                    set_heights(&mut entity.properties, &piece.heights);
                    entities.push(entity);
                }
            }
        }
    }
    if counts.degenerate != 0 {
        plan.warnings.push(format!(
            "{} result geometries had too few distinct points to draw and were left out",
            counts.degenerate
        ));
    }
    plan.skipped = counts.degenerate;
    plan.created = entities.len();
    if !entities.is_empty() {
        transaction.add(create_entities(entities));
    }
    if transaction.size() != 0 {
        plan.command = Some(Box::new(transaction));
    }
    Ok(plan)
}

/// The heights of a replaced geometry, as property edits of `entity`.
fn height_edits(entity: &Entity, heights: &[Option<f64>], transaction: &mut Transaction) {
    let mut after = entity.properties.clone();
    set_heights(&mut after, heights);
    for key in [ELEVATION_PROPERTY, ELEVATIONS_PROPERTY] {
        let before = entity.properties.get(key);
        match after.get(key) {
            Some(now) if before != Some(now) => {
                transaction.add(set_entity_property(vec![entity.id], key, now.clone()));
            }
            None if before.is_some() => {
                transaction.add(remove_entity_property(vec![entity.id], key));
            }
            _ => {}
        }
    }
}

fn in_place_plan(model: &Model, set: &FeatureSet, options: &ResultOptions) -> Result<ResultPlan> {
    let mut plan = ResultPlan::default();
    let mut transaction = transaction_named(options);
    let mut missing = 0;
    let mut several = 0;
    let mut counts = FeaturePieceCounts::default();
    for table in set.tables.iter().filter(|table| !table.features.is_empty()) {
        if !has_katana_id(table) {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "the result has no katana_id to find its entities by",
                table.name.clone(),
            ));
        }
        for feature in &table.features {
            let Some(entity) = source_of(table, feature).and_then(|id| model.entities.find(id))
            else {
                missing += 1;
                continue;
            };
            if options.mode == ResultMode::UpdateGeometry {
                if feature.parts.len() != 1
                    || (feature.parts[0].kind == GeometryKind::Polygon
                        && feature.parts[0].parts.len() > 1)
                {
                    several += 1;
                    continue;
                }
                let mut pieces = pieces_of(
                    &feature.parts[0],
                    &FeaturePieceOptions::default(),
                    &mut counts,
                );
                if pieces.len() != 1 {
                    continue;
                }
                let piece = pieces.remove(0);
                transaction.add(set_entity_geometry(entity.id, piece.geometry));
                height_edits(entity, &piece.heights, &mut transaction);
                plan.updated += 1;
                continue;
            }
            let mut changed = false;
            for (field, value) in table.fields.iter().zip(&feature.values) {
                if is_bookkeeping_field(&field.name) {
                    continue;
                }
                let Some(value) = property_of(value) else {
                    continue;
                };
                let key = format!("{}{}", options.property_prefix, field.name);
                if entity.properties.get(&key) == Some(&value) {
                    continue;
                }
                transaction.add(set_entity_property(vec![entity.id], key, value));
                changed = true;
            }
            plan.updated += usize::from(changed);
        }
    }
    if missing != 0 {
        plan.warnings.push(format!(
            "{missing} results name an entity the drawing no longer has"
        ));
    }
    if several != 0 {
        plan.warnings.push(format!(
            "{several} results of several parts cannot replace one entity and were left out"
        ));
    }
    plan.skipped = missing + several + counts.degenerate;
    if transaction.size() != 0 {
        plan.command = Some(Box::new(transaction));
    }
    Ok(plan)
}

// ---- the public functions --------------------------------------------------------------------

/// "date" or "datetime" for those two types; nothing for any other.
pub fn date_type_tag(r#type: FieldType) -> Option<String> {
    match r#type {
        FieldType::Date => Some(String::from("date")),
        FieldType::DateTime => Some(String::from("datetime")),
        _ => None,
    }
}

/// The field names drawing_dataset writes before the properties, which a
/// result does not turn back into properties: katana_id, layer, style,
/// colour, type.
pub fn is_bookkeeping_field(name: &str) -> bool {
    BOOKKEEPING.contains(&name)
}

/// The entities `ids` names, in that order, as tables. An id the model does
/// not hold is counted as skipped "missing". Never fails for what the scope
/// took; InvalidArgument for a curve tolerance that is not positive.
pub fn drawing_dataset(
    model: &Model,
    ids: &[EntityId],
    options: &DrawingDatasetOptions,
) -> Result<DrawingDataset> {
    if !(options.curve_tolerance > 0.0) || !options.curve_tolerance.is_finite() {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "the curve tolerance must be positive",
            format_exact_real(options.curve_tolerance),
        ));
    }
    let mut stats = DrawingDatasetStats {
        matched: ids.len(),
        ..DrawingDatasetStats::default()
    };
    let mut holes = BTreeMap::new();
    let mut shapes = Vec::with_capacity(ids.len());
    let mut notes = HeightNotes::default();
    for &id in ids {
        let Some(entity) = model.entities.find(id) else {
            *stats.skipped.entry(String::from("missing")).or_insert(0) += 1;
            continue;
        };
        match shape_of(entity, options, &mut notes) {
            Ok(shape) => shapes.push(shape),
            Err(reason) => *stats.skipped.entry(String::from(reason)).or_insert(0) += 1,
        }
    }
    join_holes(&mut shapes, &mut stats, &mut holes);

    stats.shifted = shapes
        .iter()
        .filter(|shape| {
            shape.kind != GeometryKind::Unknown && shape.entity.metadata.contains_key(SHIFT_KEY)
        })
        .count();
    let mut own = options.clone();
    if stats.shifted != 0 && !options.crs_wkt.is_empty() {
        own.crs_wkt.clear();
        stats.warnings.push(format!(
            "{} entities were imported moved from their file's coordinates (LOCAL, ALONGSIDE or \
             OFFSET=), so they are in no known coordinate system: the features carry none, not \
             the project's",
            stats.shifted
        ));
    }

    let points = of_kind(&shapes, GeometryKind::Point);
    let lines = of_kind(&shapes, GeometryKind::LineString);
    let polygons = of_kind(&shapes, GeometryKind::Polygon);
    stats.points = points.len();
    stats.lines = lines.len();
    stats.polygons = polygons.len();
    stats.used += points.len() + lines.len() + polygons.len();
    if notes.partial > 0 {
        stats.warnings.push(format!(
            "{} entities have heights at only some of their vertices and were written in plan, \
             their heights kept as attributes: a 3D geometry needs a height at every vertex",
            notes.partial
        ));
    }
    if notes.sloping_arcs > 0 {
        stats.warnings.push(format!(
            "{} arcs have different heights at their two ends and were written in plan, their \
             heights kept as attributes: a height between the ends of an arc is not recorded",
            notes.sloping_arcs
        ));
    }
    let mut set = FeatureSet::default();
    if own.one_table {
        let all: Vec<&Shape<'_>> = shapes
            .iter()
            .filter(|shape| shape.kind != GeometryKind::Unknown)
            .collect();
        if let Some(first) = all.first() {
            let first = first.kind;
            let uniform = all.iter().all(|shape| shape.kind == first);
            set.tables.push(table_of(
                &own.table_name,
                if uniform {
                    first
                } else {
                    GeometryKind::Unknown
                },
                &all,
                &own,
                &mut stats,
            ));
        }
        return Ok(DrawingDataset { set, stats, holes });
    }
    if !points.is_empty() {
        set.tables.push(table_of(
            "points",
            GeometryKind::Point,
            &points,
            &own,
            &mut stats,
        ));
    }
    if !lines.is_empty() {
        set.tables.push(table_of(
            "lines",
            GeometryKind::LineString,
            &lines,
            &own,
            &mut stats,
        ));
    }
    if !polygons.is_empty() {
        set.tables.push(table_of(
            "polygons",
            GeometryKind::Polygon,
            &polygons,
            &own,
            &mut stats,
        ));
    }
    Ok(DrawingDataset { set, stats, holes })
}

/// One feature geometry as the entity geometries it becomes (FeaturePiece
/// says how).
pub fn feature_pieces(
    geometry: &VectorGeometry,
    options: &FeaturePieceOptions,
    counts: &mut FeaturePieceCounts,
) -> Vec<FeaturePiece> {
    pieces_of(geometry, options, counts)
}

/// The one command that applies `set` to `model` as `options` asks. It reads
/// the model and changes nothing: the caller executes the command, which is
/// how a result becomes one undo step. InvalidArgument for a target layer
/// that is not a valid layer path, or a mode that needs katana_id on a table
/// without one.
pub fn result_command(
    model: &Model,
    set: &FeatureSet,
    options: &ResultOptions,
) -> Result<ResultPlan> {
    if options.mode == ResultMode::Create {
        create_plan(model, set, options)
    } else {
        in_place_plan(model, set, options)
    }
}
