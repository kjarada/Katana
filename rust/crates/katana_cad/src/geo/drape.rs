//! Draping the drawing on a surface or an elevation raster
//! (include/katana/cad/geo/drape.hpp; docs/terrain.md, "Sampling and
//! drape"): each point, and each vertex of a line or a polyline, is given the
//! height of the ground under it, through entity's set_heights - the one
//! writer of heights - as ONE undo step.
//!
//! Here, in the cad layer, rather than beside the verb, because what a drape
//! changes and how it is undone are the drawing's rules; where the heights
//! come from is not. The ground is a callback: the DRAPE verb hands a TIN's
//! elevation_at or a GDAL raster sampler through it, so the cad layer never
//! sees GDAL.
//!
//! The rules:
//!
//! * A point takes the height at its position; a line its two ends; a
//!   polyline every vertex (a closed one's too). A curve polyline (one with
//!   arcs) takes a height at every vertex too, written into its geometry,
//!   where it keeps its heights (docs/drawing.md): its arcs stay arcs, and the
//!   height between two vertices is the geometry's own rule, linear by
//!   length. Nothing else has vertices a height belongs to - an arc, a circle,
//!   an ellipse, a spline, a text - and those are left as they are and
//!   counted by type.
//! * A vertex off the ground, or on a hole in it, is left WITHOUT a height
//!   and counted: the drape defines the heights of what it takes, and a
//!   height kept from before would be another surface's, mixed in silently.
//!   Absent is not zero.
//! * Heights are computed first (drape_heights, pure, on a worker) and
//!   written later (drape_command, on the thread that owns the drawing), so a
//!   front end can run the sampling as a background job.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use katana_commands::{
    CommandPtr, Transaction, remove_entity_property, set_entity_geometry, set_entity_property,
};
use katana_core::text::lowered;
use katana_entity::entity::{
    ELEVATION_PROPERTY, ELEVATIONS_PROPERTY, INVALID_ENTITY_ID, set_heights,
};
use katana_entity::{Entity, EntityId, EnumName, Geometry, Model};
use katana_geometry::Point2;

/// The height of the ground at a plan position; None off it. Shared, as the
/// C++ std::function is copied: an analysis keeps one and a background job
/// runs it.
pub type HeightAt = Arc<dyn Fn(Point2) -> Option<f64> + Send + Sync>;

/// The plan positions whose heights an entity carries, in the order entity's
/// heights_of reads them: a point's position, a line's two ends, a polyline's
/// vertices - a curve polyline's in its vertex order, the order its geometry
/// holds its heights in. Empty for any other entity.
pub fn height_vertices(entity: &Entity) -> Vec<Point2> {
    match &entity.geometry {
        Geometry::Point(point) => vec![point.position],
        Geometry::Line(line) => vec![line.start, line.end],
        Geometry::Polyline(polyline) => polyline.vertices.clone(),
        Geometry::CurvePolyline(curve) => curve.positions(),
        _ => Vec::new(),
    }
}

/// The heights one entity is to be given, one per vertex.
#[derive(Clone, Debug, PartialEq)]
pub struct DrapedEntity {
    pub id: EntityId,
    pub heights: Vec<Option<f64>>,
}

impl Default for DrapedEntity {
    fn default() -> Self {
        Self {
            id: INVALID_ENTITY_ID,
            heights: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DrapeHeights {
    /// In the order they were given.
    pub entities: Vec<DrapedEntity>,
    /// Over every entity draped.
    pub vertices: usize,
    /// Of which off the ground: left heightless.
    pub vertices_off: usize,
    /// Entities no vertex of which is on the ground.
    pub entities_off: usize,
    /// Entities left as they are, by type ("arc", "circle", "text" ...).
    pub skipped: BTreeMap<String, usize>,
    /// Whether `stop` was asked before every entity was sampled; the heights
    /// are then incomplete and must not be written.
    pub stopped: bool,
}

// An entity's type as the counts name it: the C++ toString, lowered.
pub(crate) fn type_word(entity: &Entity) -> String {
    String::from_utf8_lossy(&lowered(entity.r#type().name())).into_owned()
}

/// The ground under every vertex of `entities`. Deterministic: the same
/// entities and ground give the same heights. Checks `stop` between entities
/// (the C++ std::stop_token; None is a token never stopped).
pub fn drape_heights(
    entities: &[Entity],
    ground: impl Fn(Point2) -> Option<f64>,
    stop: Option<&AtomicBool>,
) -> DrapeHeights {
    let mut out = DrapeHeights::default();
    for entity in entities {
        if stop.is_some_and(|flag| flag.load(Ordering::Acquire)) {
            out.stopped = true;
            return out;
        }
        let vertices = height_vertices(entity);
        if vertices.is_empty() {
            *out.skipped.entry(type_word(entity)).or_default() += 1;
            continue;
        }
        let mut draped = DrapedEntity {
            id: entity.id,
            heights: Vec::with_capacity(vertices.len()),
        };
        let mut any_on = false;
        for &vertex in &vertices {
            let height = ground(vertex).filter(|height| height.is_finite());
            any_on = any_on || height.is_some();
            out.vertices_off += usize::from(height.is_none());
            draped.heights.push(height);
        }
        out.vertices += vertices.len();
        out.entities_off += usize::from(!any_on);
        out.entities.push(draped);
    }
    out
}

#[derive(Default)]
pub struct DrapeCommand {
    /// None when nothing changes: every entity already has those heights.
    pub command: Option<CommandPtr>,
    /// Entities whose heights the command changes.
    pub changed: usize,
    /// Entities already at those heights.
    pub unchanged: usize,
    /// Entities the model no longer has.
    pub missing: usize,
}

/// The ONE command that gives each entity of `heights` its heights (entity's
/// set_heights on a copy of its properties, then the `elevation` /
/// `elevations` properties set or removed to match), named `name` - the verb
/// line, so the history says what ran. It reads the model and changes
/// nothing: the caller executes it, which makes the drape one undo step.
pub fn drape_command(model: &Model, heights: &[DrapedEntity], name: &str) -> DrapeCommand {
    let mut out = DrapeCommand::default();
    let mut transaction = Transaction::new(if name.is_empty() { "DRAPE" } else { name });
    for draped in heights {
        let Some(entity) = model.entities.find(draped.id) else {
            out.missing += 1;
            continue;
        };
        // A curve polyline holds its heights in its vertices (docs/drawing.md,
        // "Which kind a polyline is"), so the drape is a geometry edit: the
        // same vertices and bulges, each vertex given the ground's height.
        // Only a difference becomes an edit, as below.
        if let Geometry::CurvePolyline(curve) = &entity.geometry {
            let mut with_heights = curve.clone();
            if draped.heights.len() != with_heights.vertices.len() {
                out.missing += 1; // not the polyline the heights were sampled for
                continue;
            }
            for (vertex, &height) in with_heights.vertices.iter_mut().zip(&draped.heights) {
                vertex.height = height;
            }
            let changed = with_heights != *curve;
            if changed {
                transaction.add(set_entity_geometry(
                    entity.id,
                    Geometry::CurvePolyline(with_heights),
                ));
            }
            out.changed += usize::from(changed);
            out.unchanged += usize::from(!changed);
            continue;
        }
        // The properties as set_heights would leave them, compared key by key
        // with what the entity has: only a difference becomes an edit, so a
        // second drape on the same ground changes nothing.
        let mut after = entity.properties.clone();
        set_heights(&mut after, &draped.heights);
        let mut changed = false;
        for key in [ELEVATION_PROPERTY, ELEVATIONS_PROPERTY] {
            let before = entity.properties.get(key);
            let now = after.get(key);
            if let Some(now) = now
                && before.is_none_or(|before| before != now)
            {
                transaction.add(set_entity_property(vec![entity.id], key, now.clone()));
                changed = true;
            } else if now.is_none() && before.is_some() {
                transaction.add(remove_entity_property(vec![entity.id], key));
                changed = true;
            }
        }
        out.changed += usize::from(changed);
        out.unchanged += usize::from(!changed);
    }
    if transaction.size() != 0 {
        out.command = Some(Box::new(transaction));
    }
    out
}
