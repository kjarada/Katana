//! The concrete CAD commands. Each factory returns a ready-to-execute
//! Command; nothing touches the model until a CommandStack executes it.
//! Command names are the stable identifiers the structured command API will
//! expose.
//!
//!   CREATE_POINT CREATE_LINE CREATE_CIRCLE CREATE_ARC CREATE_POLYLINE
//!   CREATE_TEXT CREATE_DIMENSION CREATE_ENTITIES
//!   DELETE MOVE ROTATE SCALE MIRROR COPY ARRAY
//!   TRIM EXTEND OFFSET FILLET CHAMFER
//!   SET_LAYER SET_COLOR SET_STYLE SET_VISIBLE SET_PROPERTY REMOVE_PROPERTY
//!   SET_GEOMETRY
//!   CREATE_LAYER UPDATE_LAYER DELETE_LAYER
//!
//! The layer commands (layer_commands.rs) and the table managers'
//! (table_commands.rs) are declared here in the C++ and re-exported here.
//!
//! The C++ factories' defaulted arguments (the attributes, MIRROR's
//! keep-original) are passed explicitly: `EntityAttributes::default()` and
//! `false` are the C++ defaults.

use std::collections::BTreeMap;

use katana_core::error::{Error, ErrorCode, Result, Status, make_error};
use katana_core::text::format_exact_real;
use katana_entity::curve_pieces::curve_pieces;
use katana_entity::entity_geometry::transformed;
use katana_entity::{
    Color, DimensionGeometry, Entity, EntityId, EnumName, Geometry, PointGeometry, PropertyValue,
    TextGeometry,
};
use katana_geometry::curves2d::CURVE_CHORD_TOLERANCE;
use katana_geometry::editing::{chamfer, extend, fillet, offset, offset_towards, trim};
use katana_geometry::{
    Arc2, Circle2, Curve2, CurveSegment, Line2, Point2, Polyline2, Segment2, Vec2,
};
use katana_math::Mat3;
use katana_math::numerics::tolerance;

use crate::change_set::{ChangeSet, ChangeSetCommand};
use crate::command::{CommandContext, CommandPtr};

pub use crate::layer_commands::{
    create_layer, delete_layer, delete_layer_tree, rename_layer, update_layer,
};
pub use crate::table_commands::{
    create_alignment, create_dimension_style, create_hatch_pattern, create_label_rule,
    create_label_style, create_linetype, create_style, create_text_style, delete_alignment,
    delete_dimension_style, delete_hatch_pattern, delete_label_rule, delete_label_style,
    delete_linetype, delete_style, delete_text_style, duplicate_linetype, duplicate_style,
    merge_linetype, merge_style, purge_table_items, rename_linetype, rename_style,
    update_alignment, update_dimension_style, update_hatch_pattern, update_label_rule,
    update_label_style, update_linetype, update_linetype_if_changed, update_style,
    update_style_if_changed, update_text_style,
};

/// Presentation attributes given to newly created entities.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntityAttributes {
    pub layer: String,
    pub style: String,
    pub color: Option<Color>,
}

impl Default for EntityAttributes {
    fn default() -> Self {
        Self {
            layer: String::from("0"),
            style: String::new(),
            color: None,
        }
    }
}

// ---- the table managers -------------------------------------------------------------
//
// Delete, merge and purge discard definitions and say so (is_destructive).
// Every in-use refusal reads entity::table_usage and gives a count and the
// first holder: "used by 1 layer, 2 styles and 418 entities, e.g.
// layer=survey".

/// Items to delete together (purge_table_items). Judged as a set: a linetype
/// named only by styles in the same set is free.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TableItems {
    pub styles: Vec<String>,
    pub linetypes: Vec<String>,
    pub hatch_patterns: Vec<String>,
}

impl TableItems {
    pub fn empty(&self) -> bool {
        self.styles.is_empty() && self.linetypes.is_empty() && self.hatch_patterns.is_empty()
    }

    pub fn size(&self) -> usize {
        self.styles.len() + self.linetypes.len() + self.hatch_patterns.len()
    }
}

fn make_command<F>(name: impl Into<String>, builder: F, destructive: bool) -> CommandPtr
where
    F: Fn(&CommandContext<'_>) -> Result<ChangeSet> + Send + 'static,
{
    Box::new(ChangeSetCommand::new(name, builder, destructive))
}

fn id_context(id: EntityId) -> String {
    format!("id={id}")
}

fn find_entity(context: &CommandContext<'_>, id: EntityId) -> Result<Entity> {
    match context.model.entities.find(id) {
        Some(entity) => Ok(entity.clone()),
        None => Err(make_error(
            ErrorCode::NotFound,
            "entity does not exist",
            id_context(id),
        )),
    }
}

fn require_selection(ids: &[EntityId]) -> Status {
    if ids.is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "no entities were given",
        ));
    }
    Ok(())
}

fn create_one(name: &str, geometry: Geometry, attributes: EntityAttributes) -> CommandPtr {
    let entity = Entity {
        geometry,
        layer: attributes.layer,
        style: attributes.style,
        color: attributes.color,
        ..Entity::default()
    };
    make_command(
        name,
        move |_context| {
            Ok(ChangeSet {
                add: vec![entity.clone()],
                ..ChangeSet::default()
            })
        },
        false,
    )
}

// Command that rewrites each selected entity through `edit`.
fn modify_each<E>(name: impl Into<String>, ids: Vec<EntityId>, edit: E) -> CommandPtr
where
    E: Fn(&mut Entity) -> Status + Send + 'static,
{
    make_command(
        name,
        move |context| {
            require_selection(&ids)?;
            let mut changes = ChangeSet::default();
            for &id in &ids {
                let mut entity = find_entity(context, id)?;
                edit(&mut entity)?;
                changes.modify.push(entity);
            }
            Ok(changes)
        },
        false,
    )
}

fn apply_transform(entity: &mut Entity, transform: &Mat3) -> Status {
    match transformed(&entity.geometry, transform) {
        Ok(moved) => {
            entity.geometry = moved;
            Ok(())
        }
        Err(error) => Err(make_error(error.code, error.message, id_context(entity.id))),
    }
}

// Adds transformed duplicates of the selection; the originals stay.
// An annotation copied WITH the entity it refers to - a leader's tip, a
// dimension's points, a label's target - refers to that entity's copy, so a
// pit copied with its callout gets a callout of its own rather than a second
// one of the original (docs/annotation.md, "Associativity"). A reference to
// an entity not copied is kept: the copy is another annotation of it.
fn refer_to_copies(entity: &mut Entity, copy_of: &BTreeMap<EntityId, EntityId>) {
    let remap = |id: &mut u64| {
        let original: EntityId = *id;
        if let Some(&found) = copy_of.get(&original) {
            *id = found;
        }
    };
    match &mut entity.geometry {
        Geometry::Dimension(dimension) => {
            for anchor in [
                &mut dimension.start_ref,
                &mut dimension.end_ref,
                &mut dimension.vertex_ref,
            ] {
                if anchor.associated() {
                    remap(&mut anchor.entity);
                }
            }
        }
        Geometry::Leader(leader) => {
            if leader.tip_ref.associated() {
                remap(&mut leader.tip_ref.entity);
            }
        }
        Geometry::Label(label) if label.target != 0 => {
            remap(&mut label.target);
        }
        _ => {}
    }
}

fn duplicate_each(
    name: impl Into<String>,
    ids: Vec<EntityId>,
    transforms: Vec<Mat3>,
) -> CommandPtr {
    make_command(
        name,
        move |context| {
            require_selection(&ids)?;
            // The ids the copies will have: an add is given the next id, in
            // the order the change set lists them (EntityDatabase::add), and a
            // redo puts each back under the id it had.
            let first = context.model.entities.next_id();
            let mut changes = ChangeSet::default();
            for transform in &transforms {
                let mut copy_of = BTreeMap::new();
                let base = first.wrapping_add(changes.add.len() as EntityId);
                for (k, &id) in ids.iter().enumerate() {
                    copy_of.insert(id, base.wrapping_add(k as EntityId));
                }
                for &id in &ids {
                    let mut entity = find_entity(context, id)?;
                    apply_transform(&mut entity, transform)?;
                    refer_to_copies(&mut entity, &copy_of);
                    changes.add.push(entity);
                }
            }
            Ok(changes)
        },
        false,
    )
}

// ---- curve editing helpers ---------------------------------------------------------

fn as_curve(entity: &Entity) -> Result<Curve2> {
    match &entity.geometry {
        Geometry::Line(segment) => Ok(Curve2::Segment(*segment)),
        Geometry::Arc(arc) => Ok(Curve2::Arc(*arc)),
        Geometry::Circle(circle) => Ok(Curve2::Circle(*circle)),
        _ => Err(make_error(
            ErrorCode::Unsupported,
            "only lines, arcs and circles can be edited this way",
            format!("{} type={}", id_context(entity.id), entity.r#type().name()),
        )),
    }
}

fn as_geometry(curve: Curve2) -> Geometry {
    match curve {
        Curve2::Segment(segment) => Geometry::Line(segment),
        Curve2::Arc(arc) => Geometry::Arc(arc),
        Curve2::Circle(circle) => Geometry::Circle(circle),
    }
}

// Curves that act as cutting edges / boundaries. Polylines contribute
// segments.
fn edge_curves(context: &CommandContext<'_>, ids: &[EntityId]) -> Result<Vec<Curve2>> {
    let mut curves = Vec::new();
    for &id in ids {
        let entity = find_entity(context, id)?;
        match &entity.geometry {
            Geometry::Polyline(polyline) => {
                for i in 0..polyline.segment_count() {
                    curves.push(Curve2::Segment(polyline.segment(i)));
                }
            }
            // A curve polyline, an ellipse and a spline cut and bound through
            // their pieces (entity/curve_pieces.rs).
            Geometry::CurvePolyline(_) | Geometry::Ellipse(_) | Geometry::Spline(_) => {
                curves.extend(curve_pieces(&entity.geometry, CURVE_CHORD_TOLERANCE));
            }
            _ => curves.push(as_curve(&entity)?),
        }
    }
    Ok(curves)
}

fn as_segment(entity: &Entity) -> Result<Segment2> {
    match &entity.geometry {
        Geometry::Line(segment) => Ok(*segment),
        _ => Err(make_error(
            ErrorCode::Unsupported,
            "only lines can be filleted or chamfered",
            id_context(entity.id),
        )),
    }
}

// ---- creation ----------------------------------------------------------------------------

pub fn create_point(position: Point2, attributes: EntityAttributes) -> CommandPtr {
    create_one(
        "CREATE_POINT",
        Geometry::Point(PointGeometry { position }),
        attributes,
    )
}

pub fn create_line(start: Point2, end: Point2, attributes: EntityAttributes) -> CommandPtr {
    create_one(
        "CREATE_LINE",
        Geometry::Line(Segment2::new(start, end)),
        attributes,
    )
}

pub fn create_circle(center: Point2, radius: f64, attributes: EntityAttributes) -> CommandPtr {
    create_one(
        "CREATE_CIRCLE",
        Geometry::Circle(Circle2::new(center, radius)),
        attributes,
    )
}

pub fn create_arc(arc: Arc2, attributes: EntityAttributes) -> CommandPtr {
    create_one("CREATE_ARC", Geometry::Arc(arc), attributes)
}

pub fn create_polyline(polyline: Polyline2, attributes: EntityAttributes) -> CommandPtr {
    create_one("CREATE_POLYLINE", Geometry::Polyline(polyline), attributes)
}

pub fn create_text(text: TextGeometry, attributes: EntityAttributes) -> CommandPtr {
    create_one("CREATE_TEXT", Geometry::Text(text), attributes)
}

pub fn create_dimension(dimension: DimensionGeometry, attributes: EntityAttributes) -> CommandPtr {
    create_one(
        "CREATE_DIMENSION",
        Geometry::Dimension(dimension),
        attributes,
    )
}

/// Adds fully specified entities (imports, paste). Their ids are ignored.
pub fn create_entities(entities: Vec<Entity>) -> CommandPtr {
    // The copy here is deliberate and cannot become a move. A builder is a
    // pure function of the model ("validate() has no side effects and may be
    // called at any time"), and validate() really is called twice on the
    // first command of a Transaction - once by Transaction::validate and again
    // by Transaction::execute. A builder that moved its captured entities out
    // would hand the second call an empty set, and the command would be
    // rejected as one that "would not change anything". ChangeSetCommand keeps
    // what validate() built and hands it to execute(), so this copy is made
    // once per command rather than once per call, and execute() then moves
    // the entities into the database instead of copying them again.
    make_command(
        "CREATE_ENTITIES",
        move |_context| {
            Ok(ChangeSet {
                add: entities.clone(),
                ..ChangeSet::default()
            })
        },
        false,
    )
}

// ---- removal / transformation ------------------------------------------------------------

pub fn delete_entities(ids: Vec<EntityId>) -> CommandPtr {
    make_command(
        "DELETE",
        move |_context| {
            require_selection(&ids)?;
            Ok(ChangeSet {
                remove: ids.clone(),
                ..ChangeSet::default()
            })
        },
        true,
    )
}

/// Any similarity transform.
pub fn transform_entities(
    name: impl Into<String>,
    ids: Vec<EntityId>,
    transform: &Mat3,
) -> CommandPtr {
    let transform = *transform;
    modify_each(name, ids, move |entity| apply_transform(entity, &transform))
}

pub fn move_entities(ids: Vec<EntityId>, delta: Vec2) -> CommandPtr {
    transform_entities("MOVE", ids, &Mat3::translation(delta))
}

pub fn rotate_entities(ids: Vec<EntityId>, center: Point2, radians: f64) -> CommandPtr {
    transform_entities("ROTATE", ids, &Mat3::rotation_about(center, radians))
}

/// Uniform scale about `center`; factor must be positive and finite.
pub fn scale_entities(ids: Vec<EntityId>, center: Point2, factor: f64) -> CommandPtr {
    if !(factor.is_finite() && factor > 0.0) {
        return make_command(
            "SCALE",
            move |_context| {
                Err(make_error(
                    ErrorCode::InvalidArgument,
                    "scale factor must be positive",
                    format_exact_real(factor),
                ))
            },
            false,
        );
    }
    transform_entities("SCALE", ids, &Mat3::scaling_about(center, factor, factor))
}

/// Mirror about the line through a and b. `keep_original` (false in the C++
/// default) adds mirrored copies instead.
pub fn mirror_entities(
    ids: Vec<EntityId>,
    a: Point2,
    b: Point2,
    keep_original: bool,
) -> CommandPtr {
    if a.distance_to(b) <= tolerance::GEOMETRIC {
        return make_command(
            "MIRROR",
            |_context| {
                Err(Error::new(
                    ErrorCode::InvalidArgument,
                    "mirror line needs two distinct points",
                ))
            },
            false,
        );
    }
    let reflection = Mat3::reflection(a, b - a);
    if keep_original {
        return duplicate_each("MIRROR", ids, vec![reflection]);
    }
    transform_entities("MIRROR", ids, &reflection)
}

pub fn copy_entities(ids: Vec<EntityId>, delta: Vec2) -> CommandPtr {
    duplicate_each("COPY", ids, vec![Mat3::translation(delta)])
}

/// Copies of `ids`, one set per transform, as ONE command named `name`: what
/// COPY, MIRROR (keeping the original) and ARRAY make, and the window's Copy,
/// Rotate and Scale with Copy, and polar Array tools. An annotation copied
/// with the entity it refers to refers to that entity's copy
/// (docs/annotation.md, "Associativity"). Refused with no ids or no transform.
pub fn duplicate_entities(
    name: impl Into<String>,
    ids: Vec<EntityId>,
    transforms: Vec<Mat3>,
) -> CommandPtr {
    if transforms.is_empty() {
        return make_command(
            name,
            |_context| Err(Error::new(ErrorCode::InvalidArgument, "nothing to copy")),
            false,
        );
    }
    duplicate_each(name, ids, transforms)
}

/// Rectangular array: rows x columns copies spaced by `spacing` (the originals
/// occupy cell 0,0 and are kept).
pub fn array_entities(ids: Vec<EntityId>, rows: i32, columns: i32, spacing: Vec2) -> CommandPtr {
    const MAXIMUM_COPIES: i64 = 100_000; // guards against a runaway typo, not a design limit
    if rows < 1
        || columns < 1
        || i64::from(rows) * i64::from(columns) > MAXIMUM_COPIES
        || (rows == 1 && columns == 1)
    {
        return make_command(
            "ARRAY",
            move |_context| {
                Err(make_error(
                    ErrorCode::InvalidArgument,
                    "array needs at least 1x2 cells and at most 100000",
                    format!("{rows}x{columns}"),
                ))
            },
            false,
        );
    }
    let mut transforms = Vec::new();
    for row in 0..rows {
        for column in 0..columns {
            if row != 0 || column != 0 {
                transforms.push(Mat3::translation(Vec2::new(
                    spacing.x * f64::from(column),
                    spacing.y * f64::from(row),
                )));
            }
        }
    }
    duplicate_each("ARRAY", ids, transforms)
}

// ---- curve editing (targets must be Line, Arc or Circle entities) ---------------
// Cutters / boundaries may also be polylines; each of their segments is used.

pub fn trim_entity(target: EntityId, cutters: Vec<EntityId>, pick: Point2) -> CommandPtr {
    make_command(
        "TRIM",
        move |context| {
            let entity = find_entity(context, target)?;
            let curve = as_curve(&entity)?;
            let edges = edge_curves(context, &cutters)?;
            let trimmed = trim(&curve, &edges, pick)?;
            let mut changes = ChangeSet::default();
            let Some((first_piece, further)) = trimmed.remaining.split_first() else {
                changes.remove.push(target);
                return Ok(changes);
            };
            // The first piece keeps the identity; further pieces are new
            // entities inheriting every attribute of the original.
            let mut first = entity.clone();
            first.geometry = as_geometry(*first_piece);
            changes.modify.push(first);
            for piece_curve in further {
                let mut piece = entity.clone();
                piece.geometry = as_geometry(*piece_curve);
                changes.add.push(piece);
            }
            Ok(changes)
        },
        false,
    )
}

pub fn extend_entity(target: EntityId, boundaries: Vec<EntityId>, pick: Point2) -> CommandPtr {
    make_command(
        "EXTEND",
        move |context| {
            let mut entity = find_entity(context, target)?;
            let curve = as_curve(&entity)?;
            let edges = edge_curves(context, &boundaries)?;
            let extended = extend(&curve, &edges, pick)?;
            entity.geometry = as_geometry(extended);
            Ok(ChangeSet {
                modify: vec![entity],
                ..ChangeSet::default()
            })
        },
        false,
    )
}

/// Creates an offset copy on the side of `side`. Also accepts polylines.
pub fn offset_entity(source: EntityId, distance: f64, side: Point2) -> CommandPtr {
    make_command(
        "OFFSET",
        move |context| {
            let mut entity = find_entity(context, source)?;
            let geometry = match &entity.geometry {
                Geometry::Polyline(polyline) => {
                    // Side of the nearest segment decides the sign (left is
                    // positive).
                    let mut sign = 1.0;
                    let mut best = f64::INFINITY;
                    for i in 0..polyline.segment_count() {
                        let segment = polyline.segment(i);
                        let d = segment.distance_to(side);
                        if d < best && !segment.is_degenerate() {
                            best = d;
                            let line = Line2 {
                                origin: segment.start,
                                direction: segment.delta(),
                            };
                            sign = if line.signed_distance_to(side) >= 0.0 {
                                1.0
                            } else {
                                -1.0
                            };
                        }
                    }
                    Geometry::Polyline(offset(polyline, sign * distance.abs())?)
                }
                Geometry::CurvePolyline(curved) => {
                    // The side of the nearest piece decides the sign: left of
                    // a line, or - on an arc - inside a counter-clockwise one.
                    let Some(near) = curved.nearest(side) else {
                        return Err(Error::new(
                            ErrorCode::InvalidGeometry,
                            "the polyline has no vertices",
                        ));
                    };
                    let sign = match curved.segment(near.segment) {
                        CurveSegment::Segment(piece) => {
                            let line = Line2 {
                                origin: piece.start,
                                direction: piece.delta(),
                            };
                            if line.signed_distance_to(side) >= 0.0 {
                                1.0
                            } else {
                                -1.0
                            }
                        }
                        CurveSegment::Arc(arc) => {
                            let inside = side.distance_to(arc.center) < arc.radius;
                            if inside == (arc.sweep > 0.0) {
                                1.0
                            } else {
                                -1.0
                            }
                        }
                    };
                    Geometry::CurvePolyline(offset(curved, sign * distance.abs())?)
                }
                _ => {
                    let curve = as_curve(&entity)?;
                    as_geometry(offset_towards(&curve, distance, side)?)
                }
            };
            entity.geometry = geometry;
            Ok(ChangeSet {
                add: vec![entity],
                ..ChangeSet::default()
            })
        },
        false,
    )
}

pub fn fillet_entities(first: EntityId, second: EntityId, radius: f64) -> CommandPtr {
    make_command(
        "FILLET",
        move |context| {
            if first == second {
                return Err(Error::new(
                    ErrorCode::InvalidArgument,
                    "fillet needs two different lines",
                ));
            }
            let mut a = find_entity(context, first)?;
            let mut b = find_entity(context, second)?;
            let segment_a = as_segment(&a)?;
            let segment_b = as_segment(&b)?;
            let result = fillet(&segment_a, &segment_b, radius)?;
            let mut changes = ChangeSet::default();
            if let Some(arc) = result.arc {
                let mut join = a.clone(); // the fillet arc inherits the first line's attributes
                join.geometry = Geometry::Arc(arc);
                changes.add.push(join);
            }
            a.geometry = Geometry::Line(result.first);
            b.geometry = Geometry::Line(result.second);
            changes.modify.push(a);
            changes.modify.push(b);
            Ok(changes)
        },
        false,
    )
}

pub fn chamfer_entities(
    first: EntityId,
    second: EntityId,
    distance_first: f64,
    distance_second: f64,
) -> CommandPtr {
    make_command(
        "CHAMFER",
        move |context| {
            if first == second {
                return Err(Error::new(
                    ErrorCode::InvalidArgument,
                    "chamfer needs two different lines",
                ));
            }
            let mut a = find_entity(context, first)?;
            let mut b = find_entity(context, second)?;
            let segment_a = as_segment(&a)?;
            let segment_b = as_segment(&b)?;
            let result = chamfer(&segment_a, &segment_b, distance_first, distance_second)?;
            let mut changes = ChangeSet::default();
            if let Some(bevel) = result.bevel {
                let mut join = a.clone();
                join.geometry = Geometry::Line(bevel);
                changes.add.push(join);
            }
            a.geometry = Geometry::Line(result.first);
            b.geometry = Geometry::Line(result.second);
            changes.modify.push(a);
            changes.modify.push(b);
            Ok(changes)
        },
        false,
    )
}

// ---- attributes -------------------------------------------------------------------------------

pub fn set_entity_layer(ids: Vec<EntityId>, layer: impl Into<String>) -> CommandPtr {
    let layer = layer.into();
    modify_each("SET_LAYER", ids, move |entity| {
        entity.layer = layer.clone();
        Ok(())
    })
}

pub fn set_entity_color(ids: Vec<EntityId>, color: Option<Color>) -> CommandPtr {
    modify_each("SET_COLOR", ids, move |entity| {
        entity.color = color;
        Ok(())
    })
}

pub fn set_entity_style(ids: Vec<EntityId>, style: impl Into<String>) -> CommandPtr {
    let style = style.into();
    modify_each("SET_STYLE", ids, move |entity| {
        entity.style = style.clone();
        Ok(())
    })
}

pub fn set_entity_visible(ids: Vec<EntityId>, visible: bool) -> CommandPtr {
    modify_each("SET_VISIBLE", ids, move |entity| {
        entity.visible = visible;
        Ok(())
    })
}

pub fn set_entity_property(
    ids: Vec<EntityId>,
    key: impl Into<String>,
    value: PropertyValue,
) -> CommandPtr {
    let key = key.into();
    modify_each("SET_PROPERTY", ids, move |entity| {
        if key.is_empty() {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "property name is empty",
            ));
        }
        entity.properties.insert(key.clone(), value.clone());
        Ok(())
    })
}

pub fn remove_entity_property(ids: Vec<EntityId>, key: impl Into<String>) -> CommandPtr {
    let key = key.into();
    modify_each("REMOVE_PROPERTY", ids, move |entity| {
        if entity.properties.remove(&key).is_none() {
            return Err(make_error(
                ErrorCode::NotFound,
                "entity does not have this property",
                format!("{} key={key}", id_context(entity.id)),
            ));
        }
        Ok(())
    })
}

/// Renames a property on every entity that has it, keeping its value and its
/// type. Refuses a target the entity already has: the two values differ, and
/// silently keeping one of them would lose the other. An archive import
/// names properties after the attribute tree it flattened
/// ("Asset/Dimensions/Size"), which is the first thing a user wants to tidy.
pub fn rename_entity_property(
    ids: Vec<EntityId>,
    from: impl Into<String>,
    to: impl Into<String>,
) -> CommandPtr {
    let from = from.into();
    let to = to.into();
    modify_each("RENAME_PROPERTY", ids, move |entity| {
        if to.is_empty() {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "property name is empty",
            ));
        }
        let Some(value) = entity.properties.get(&from).cloned() else {
            return Err(make_error(
                ErrorCode::NotFound,
                "entity does not have this property",
                format!("{} key={from}", id_context(entity.id)),
            ));
        };
        if from != to && entity.properties.contains_key(&to) {
            return Err(make_error(
                ErrorCode::AlreadyExists,
                "the entity already has a property of that name",
                format!("{} key={to}", id_context(entity.id)),
            ));
        }
        entity.properties.remove(&from);
        entity.properties.insert(to.clone(), value);
        Ok(())
    })
}

/// Replaces the geometry of one entity (property editor, grip editing).
pub fn set_entity_geometry(id: EntityId, geometry: Geometry) -> CommandPtr {
    modify_each("SET_GEOMETRY", vec![id], move |entity| {
        entity.geometry = geometry.clone();
        Ok(())
    })
}
