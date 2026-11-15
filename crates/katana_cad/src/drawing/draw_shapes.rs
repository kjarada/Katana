//! The arithmetic and the commands the draw tools and the draw verbs share
//! (docs/drawing.md, "The draw tools" and "The command line";
//! include/katana/cad/drawing/draw_shapes.hpp,
//! src/katana_cad/drawing/draw_shapes.cpp): the tangent arc a polyline's next
//! arc segment is, the construction line or ray a base point and a direction
//! make, the two sides of a double line, and the ONE command that creates
//! what was drawn. A tool collects points from clicks and a verb from its
//! arguments; what they make is decided here, so PLINE ... ARC ... typed by
//! an agent and the Polyline tool's Arc mode draw the same arc.

use katana_commands::{
    ChangeSet, ChangeSetCommand, CommandPtr, EntityAttributes, Transaction, create_layer,
};
use katana_entity::{Color, Entity, Geometry, Layer, Model};
use katana_geometry::curves2d::bulge_from_sweep;
use katana_geometry::editing::offset;
use katana_geometry::{CurvePolyline2, Point2, Polyline2, Segment2, Vec2};
use katana_math::numerics::tolerance;

use crate::drawing::construction::{CONSTRUCTION_LAYER, CONSTRUCTION_REACH};

/// The direction of travel at the end of `path`'s last segment - an arc's end
/// tangent, not its chord - or +x when it has fewer than two vertices.
pub fn end_heading(path: &CurvePolyline2) -> Vec2 {
    let count = path.vertices.len();
    if count < 2 {
        return Vec2::new(1.0, 0.0);
    }
    let from = &path.vertices[count - 2];
    let chord = (path.vertices[count - 1].position - from.position).normalized();
    // A segment's end tangent is its chord turned by half its sweep.
    let sweep = 4.0 * from.bulge.atan();
    chord.rotated(0.5 * sweep)
}

/// The bulge of the arc from `path`'s last vertex to `to` that leaves tangent
/// to end_heading(): the chord makes half the sweep with the tangent. 0 for
/// an empty path or a `to` on the last vertex.
pub fn tangent_bulge(path: &CurvePolyline2, to: Point2) -> f64 {
    let Some(last) = path.vertices.last() else {
        return 0.0;
    };
    let chord = to - last.position;
    if chord.length() <= tolerance::GEOMETRIC {
        return 0.0;
    }
    let tangent = end_heading(path);
    let half = tangent.cross(chord).atan2(tangent.dot(chord));
    bulge_from_sweep(2.0 * half)
}

/// A construction line through `through` along `direction` (a unit vector),
/// CONSTRUCTION_REACH either way, or a ray from it the same reach forward.
pub fn construction_segment(through: Point2, direction: Vec2, ray: bool) -> Segment2 {
    let far = through + direction * CONSTRUCTION_REACH;
    if ray {
        Segment2::new(through, far)
    } else {
        Segment2::new(through - direction * CONSTRUCTION_REACH, far)
    }
}

/// ONE command that creates `lines` on the construction layer - named
/// CREATE_RAY or CREATE_XLINE - and the layer first when `model` has none, so
/// the aid and its layer are one undo step.
pub fn create_construction(
    model: &Model,
    mut attributes: EntityAttributes,
    lines: &[Segment2],
    ray: bool,
) -> CommandPtr {
    let name = if ray { "CREATE_RAY" } else { "CREATE_XLINE" };
    attributes.layer = String::from(CONSTRUCTION_LAYER);
    let mut transaction = Transaction::new(name);
    if model.layers.find(&attributes.layer).is_none() {
        transaction.add(create_layer(Layer {
            name: attributes.layer.clone(),
            color: Color::new(0x80, 0x80, 0x80, 0xFF),
            ..Layer::default()
        }));
    }
    let entities = lines
        .iter()
        .map(|line| drawn_entity(Geometry::Line(*line), &attributes))
        .collect();
    transaction.add(create_drawn(name, entities));
    Box::new(transaction)
}

/// The two sides of a double line of `width` along `path`, left then right;
/// fewer than two when the path is too tight for the width to offset.
pub fn double_line_sides(path: &Polyline2, width: f64) -> Vec<Polyline2> {
    let mut out = Vec::new();
    if path.vertices.len() < 2 || !(width > 0.0) {
        return out;
    }
    for sign in [1.0, -1.0] {
        if let Ok(side) = offset(path, sign * 0.5 * width) {
            out.push(side);
        }
    }
    out
}

/// An entity of `geometry` in `attributes`' layer, style and colour.
pub fn drawn_entity(geometry: Geometry, attributes: &EntityAttributes) -> Entity {
    Entity {
        geometry,
        layer: attributes.layer.clone(),
        style: attributes.style.clone(),
        color: attributes.color,
        ..Entity::default()
    }
}

/// ONE command named `name` that adds `entities`.
pub fn create_drawn(name: impl Into<String>, entities: Vec<Entity>) -> CommandPtr {
    Box::new(ChangeSetCommand::new(
        name,
        move |_context| {
            Ok(ChangeSet {
                add: entities.clone(),
                ..ChangeSet::default()
            })
        },
        false,
    ))
}
