//! The values a label (label_values.rs) and a leader (leader_values.rs) both
//! read off an entity, worked out in ONE place so that a label and a leader on
//! the same line cannot print two bearings for it. Internal to the crate.

use katana_geometry::{Arc2, Point2, Vec2};
use katana_math::numerics::{PI, TWO_PI, normalize_angle, tolerance};

use crate::entity::{Entity, PropertyValue, property_to_string};
use crate::label_text::{LabelQuantity, LabelValue, LabelValues};

fn put(values: &mut LabelValues, name: &str, value: LabelValue) {
    values.insert(name.to_owned(), value);
}

/// A whole-circle bearing: clockwise from grid north (+y), [0, 2 pi).
pub(crate) fn bearing_of(along: Vec2) -> f64 {
    normalize_angle(along.x.atan2(along.y))
}

/// id, layer, code (the first of `code_properties` the entity carries, in its
/// properties or its metadata), prop.NAME for every property, and point and
/// description as text.
pub(crate) fn add_common_values(
    entity: &Entity,
    code_properties: &[String],
    values: &mut LabelValues,
) {
    put(
        values,
        "id",
        LabelValue::of(LabelQuantity::Integer, entity.id as f64),
    );
    put(values, "layer", LabelValue::of_text(entity.layer.clone()));
    // The first code property the entity carries, as text.
    for key in code_properties {
        let found = entity
            .properties
            .get(key)
            .or_else(|| entity.metadata.get(key));
        if let Some(found) = found {
            put(
                values,
                "code",
                LabelValue::of_text(property_to_string(found)),
            );
            break;
        }
    }
    for (key, value) in &entity.properties {
        let converted = match value {
            PropertyValue::Text(text) => LabelValue::of_text(text.clone()),
            PropertyValue::Real(real) => LabelValue::of(LabelQuantity::Number, *real),
            PropertyValue::Integer(integer) => {
                LabelValue::of(LabelQuantity::Integer, *integer as f64)
            }
            PropertyValue::Boolean(_) => LabelValue::of_text(property_to_string(value)),
        };
        values.insert(format!("prop.{key}"), converted);
        // The two properties survey import writes that a label names directly,
        // as text whatever their stored type - point 1001 is a name, not a
        // quantity.
        if key == "point" || key == "description" {
            put(values, key, LabelValue::of_text(property_to_string(value)));
        }
    }
}

/// A straight piece from `from` to `to`, segment `index` (0-based; printed
/// 1-based): bearing (when it has a direction), distance, length, dx, dy,
/// segment, and dz and grade when both ends have a height.
pub(crate) fn add_segment_values(
    values: &mut LabelValues,
    from: Point2,
    to: Point2,
    index: usize,
    z_from: Option<f64>,
    z_to: Option<f64>,
) {
    let along = to - from;
    let length = along.length();
    // A piece of no length has no direction: its bearing is absent rather
    // than the 0 atan2 would give it.
    if length > tolerance::GEOMETRIC {
        put(
            values,
            "bearing",
            LabelValue::of(LabelQuantity::Bearing, bearing_of(along)),
        );
    }
    put(
        values,
        "distance",
        LabelValue::of(LabelQuantity::Length, length),
    );
    put(
        values,
        "length",
        LabelValue::of(LabelQuantity::Length, length),
    );
    put(values, "dx", LabelValue::of(LabelQuantity::Length, along.x));
    put(values, "dy", LabelValue::of(LabelQuantity::Length, along.y));
    put(
        values,
        "segment",
        LabelValue::of(LabelQuantity::Integer, (index + 1) as f64),
    );
    if let (Some(z_from), Some(z_to)) = (z_from, z_to) {
        let dz = z_to - z_from;
        put(values, "dz", LabelValue::of(LabelQuantity::Number, dz));
        if length > tolerance::GEOMETRIC {
            // Per cent, as a road or a drain is graded.
            put(
                values,
                "grade",
                LabelValue::of(LabelQuantity::Number, 100.0 * dz / length),
            );
        }
    }
}

/// An arc: radius, length, delta, and chord, bearing (of the chord) and
/// tangent where they mean something (docs/annotation.md).
pub(crate) fn add_arc_values(values: &mut LabelValues, arc: &Arc2) {
    let delta = arc.sweep.abs();
    put(
        values,
        "radius",
        LabelValue::of(LabelQuantity::Length, arc.radius),
    );
    put(
        values,
        "length",
        LabelValue::of(LabelQuantity::Length, arc.length()),
    );
    put(values, "delta", LabelValue::of(LabelQuantity::Angle, delta));
    let full_turn = delta >= TWO_PI - tolerance::ANGULAR;
    if !full_turn {
        let chord = arc.point_at(1.0) - arc.point_at(0.0);
        put(
            values,
            "chord",
            LabelValue::of(LabelQuantity::Length, chord.length()),
        );
        put(
            values,
            "bearing",
            LabelValue::of(LabelQuantity::Bearing, bearing_of(chord)),
        );
        // The tangent length R tan(delta / 2): from each end to where the
        // tangents meet. Past half a turn they meet behind the curve and the
        // figure is not what anyone calls a tangent length, so it is absent.
        if delta < PI - tolerance::ANGULAR {
            put(
                values,
                "tangent",
                LabelValue::of(LabelQuantity::Length, arc.radius * (0.5 * delta).tan()),
            );
        }
    }
}
