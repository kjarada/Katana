//! Vertex editing of polyline ENTITIES (include/katana/cad/drawing/vertex_editing.hpp):
//! the layer between the pure arithmetic of geometry's polyline_vertices and
//! everything that edits a drawing's polylines vertex by vertex - the grips,
//! the Draw > Vertices tools, the Vertices panel and the VERTEX verbs
//! (docs/drawing.md).
//!
//! A polyline entity is either kind (docs/drawing.md, "Which kind a polyline
//! is"): a Polyline2 with its heights in the elevation properties, or a
//! CurvePolyline2 holding bulges and heights itself. read_polyline() makes
//! either one CurvePolyline2; write_polyline() puts an edited one back as the
//! simplest kind that holds it, moving the heights to wherever that kind
//! keeps them. So an edit never needs to know which kind it was given, and
//! straightening the last arc of a string gives back the Polyline2 every
//! other tool works on.
//!
//! Every command here is ONE ChangeSetCommand - one undo step - named by what
//! it does (VERTEX_INSERT, WEED, ...), because the undo menu and the command
//! log show the name.

use katana_commands::{ChangeSet, ChangeSetCommand, CommandContext, CommandPtr};
use katana_core::{Error, ErrorCode, Result};
use katana_entity::entity::{heights_of, set_heights, to_string};
use katana_entity::{Entity, EntityId, Geometry, Model, validate};
use katana_geometry::polyline_vertices::vertices_between;
use katana_geometry::{Box2, CurvePolyline2, Point2, Polyline2, PolylineResult};
use katana_math::cpp;

use crate::document::Document;
use crate::layer_overrides::LayerOverrides;
use crate::selection::is_drawn;
use crate::survey_import::SurveyImportOptions;
use crate::tools::everyday_support::fixed;

/// How near a survey point must lie to a vertex to be its (the C++ default
/// tolerance of verticesOnSurveyPoints).
pub const SURVEY_POINT_TOLERANCE: f64 = 1.0e-4;

/// An edit of one polyline as a function of its vertices, boxed for a caller
/// that keeps one (edit_polyline takes any such closure).
pub type PolylineEdit = Box<dyn Fn(&CurvePolyline2) -> PolylineResult + Send>;

/// An edit that depends on which polyline it is given (Weed keeping each
/// one's own survey-point vertices), boxed for a caller that keeps one.
pub type PolylineEditOf = Box<dyn Fn(&Model, EntityId, &CurvePolyline2) -> PolylineResult + Send>;

/// True for the two polyline kinds.
pub fn is_polyline_entity(entity: &Entity) -> bool {
    matches!(
        entity.geometry,
        Geometry::Polyline(_) | Geometry::CurvePolyline(_)
    )
}

/// The entity's polyline as one CurvePolyline2 (a Polyline2's heights read
/// through entity's heights_of); None for any other kind.
pub fn read_polyline(entity: &Entity) -> Option<CurvePolyline2> {
    match &entity.geometry {
        Geometry::Polyline(straight) => Some(CurvePolyline2::from_polyline(
            straight,
            &heights_of(&entity.properties, straight.vertices.len()),
        )),
        Geometry::CurvePolyline(curved) => Some(curved.clone()),
        _ => None,
    }
}

/// `entity` with `polyline` as its geometry, stored as the simplest kind: a
/// Polyline2 (heights to the properties through entity's set_heights) when
/// no segment is an arc, else a CurvePolyline2 (the elevation properties
/// removed, since the geometry holds the heights). Fails with InvalidGeometry
/// when the polyline is not one the model accepts.
pub fn write_polyline(mut entity: Entity, polyline: &CurvePolyline2) -> Result<Entity> {
    if polyline.has_arcs() {
        entity.geometry = Geometry::CurvePolyline(polyline.clone());
        // The geometry holds the heights now; a stale property list would be
        // read by anything that asks the properties (the rule has one home).
        set_heights(&mut entity.properties, &[]);
    } else {
        entity.geometry = Geometry::Polyline(Polyline2::new(polyline.positions(), polyline.closed));
        set_heights(&mut entity.properties, &polyline.heights());
    }
    validate(&entity.geometry)?;
    Ok(entity)
}

fn apply_edit(
    model: &Model,
    id: EntityId,
    edit: &dyn Fn(&CurvePolyline2) -> PolylineResult,
) -> Result<Entity> {
    let Some(found) = model.entities.find(id) else {
        return Err(Error::new(
            ErrorCode::NotFound,
            format!("there is no entity {id}"),
        ));
    };
    let Some(polyline) = read_polyline(found) else {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            format!(
                "entity {id} is a {}, not a polyline; its vertices cannot be edited",
                to_string(found.r#type())
            ),
        ));
    };
    let edited = edit(&polyline)?;
    write_polyline(found.clone(), &edited)
}

/// ONE undoable command that applies `edit` to polyline `id`. Refused, with
/// the edit's own sentence, when the entity is not a polyline or the edit
/// fails; the drawing is untouched either way.
pub fn edit_polyline(
    id: EntityId,
    name: impl Into<String>,
    edit: impl Fn(&CurvePolyline2) -> PolylineResult + Send + 'static,
) -> CommandPtr {
    Box::new(ChangeSetCommand::new(
        name,
        move |context: &CommandContext<'_>| {
            let entity = apply_edit(&*context.model, id, &edit)?;
            Ok(ChangeSet {
                modify: vec![entity],
                ..ChangeSet::default()
            })
        },
        false,
    ))
}

/// The same edit on several polylines as ONE command (Weed on a selection).
pub fn edit_polylines(
    ids: Vec<EntityId>,
    name: impl Into<String>,
    edit: impl Fn(&CurvePolyline2) -> PolylineResult + Send + 'static,
) -> CommandPtr {
    Box::new(ChangeSetCommand::new(
        name,
        move |context: &CommandContext<'_>| {
            let mut changes = ChangeSet::default();
            for &id in &ids {
                changes.modify.push(apply_edit(&*context.model, id, &edit)?);
            }
            Ok(changes)
        },
        false,
    ))
}

/// An edit that depends on which polyline it is given, on several polylines
/// as ONE command.
pub fn edit_each_polyline(
    ids: Vec<EntityId>,
    name: impl Into<String>,
    edit: impl Fn(&Model, EntityId, &CurvePolyline2) -> PolylineResult + Send + 'static,
) -> CommandPtr {
    Box::new(ChangeSetCommand::new(
        name,
        move |context: &CommandContext<'_>| {
            let model: &Model = &*context.model;
            let mut changes = ChangeSet::default();
            for &id in &ids {
                let entity = apply_edit(model, id, &|polyline: &CurvePolyline2| {
                    edit(model, id, polyline)
                })?;
                changes.modify.push(entity);
            }
            Ok(changes)
        },
        false,
    ))
}

/// Which vertices of `polyline` carry survey point data: a survey point (a
/// point entity with the survey import's point-number property) lies on the
/// vertex within `tolerance` (SURVEY_POINT_TOLERANCE is the C++ default).
/// What Weed's "keep survey points" option keeps.
pub fn vertices_on_survey_points(
    model: &Model,
    polyline: &CurvePolyline2,
    tolerance: f64,
) -> Vec<bool> {
    let mut keep = vec![false; polyline.vertices.len()];
    let property = SurveyImportOptions::default().point_number_property;
    let mut marks: Vec<Point2> = Vec::new();
    model.entities.for_each(|entity| {
        if let Geometry::Point(point) = &entity.geometry
            && entity.properties.contains_key(&property)
        {
            marks.push(point.position);
        }
    });
    for (kept, vertex) in keep.iter_mut().zip(&polyline.vertices) {
        if marks
            .iter()
            .any(|mark| mark.distance_to(vertex.position) <= tolerance)
        {
            *kept = true;
        }
    }
    keep
}

/// The same for a document's drawing (the C++ overload of a Document).
pub fn vertices_on_survey_points_in(
    document: &Document,
    polyline: &CurvePolyline2,
    tolerance: f64,
) -> Vec<bool> {
    vertices_on_survey_points(document.model(), polyline, tolerance)
}

/// The height the drawing has at `at`: a point's, a polyline's vertex's or a
/// line's end's lying within `tolerance`, the nearest first, of what `view`
/// DRAWS (selection.rs, is_drawn: a locked layer's heights are still read);
/// None when nothing with a height is there. What a 3D polyline takes from a
/// snapped point and Set Vertex Height from a click. The view has no default,
/// as is_drawn's has none: a hidden point's height once went into a vertex
/// the user could see, from a point they could not.
pub fn height_at_point(
    document: &Document,
    at: Point2,
    tolerance: f64,
    view: &LayerOverrides,
) -> Option<f64> {
    let mut best = None;
    let mut best_distance = f64::INFINITY;
    let mut offer = |p: Point2, height: Option<f64>| {
        let d = p.distance_to(at);
        if height.is_some() && d <= tolerance && d < best_distance {
            best_distance = d;
            best = height;
        }
    };
    let mut ids = Vec::new();
    document.spatial_index().query_into(
        Box2::new(at, at).inflated(cpp::max(tolerance, 1.0e-9)),
        &mut ids,
    );
    for id in ids {
        let Some(entity) = document.model().entities.find(id) else {
            continue;
        };
        if !is_drawn(document.model(), entity, view) {
            continue;
        }
        match &entity.geometry {
            Geometry::Point(point) => offer(point.position, heights_of(&entity.properties, 1)[0]),
            Geometry::Line(line) => {
                let heights = heights_of(&entity.properties, 2);
                offer(line.start, heights[0]);
                offer(line.end, heights[1]);
            }
            _ => {
                if let Some(polyline) = read_polyline(entity) {
                    for vertex in &polyline.vertices {
                        offer(vertex.position, vertex.height);
                    }
                }
            }
        }
    }
    best
}

/// The two vertices of a Straighten or a Grade in the order the arithmetic
/// walks them (geometry's straighten and grade_between walk FORWARD from the
/// first). An open polyline's lower first. A closed polyline's either way
/// round: forward from `a` unless the other way takes in fewer vertices (a
/// tie walks forward), and the other way round with `other_side`. The one
/// rule of the window's Straighten and Grade tools (their O is `other_side`)
/// and of the STRAIGHTEN and VERTEXZ GRADE verbs' side=short and side=long,
/// so picks 3 then 1 of a hexagon and "STRAIGHTEN id 3 1 side=short" take
/// out the same vertex. The verbs' default stays their forward walk from the
/// first number.
pub fn vertex_range(
    polyline: &CurvePolyline2,
    a: usize,
    b: usize,
    other_side: bool,
) -> (usize, usize) {
    if !polyline.closed {
        return (a.min(b), a.max(b));
    }
    let forward = vertices_between(polyline, a, b).len();
    let backward = vertices_between(polyline, b, a).len();
    let mut walk_forward = forward <= backward;
    if other_side {
        walk_forward = !walk_forward;
    }
    if walk_forward { (a, b) } else { (b, a) }
}

/// A vertex's height as the vertex tools' labels and the grips' hover hint
/// write it: "z 101.500", or "no height". One writer, so a vertex never reads
/// "z -0.000" in the band and "z 0.000" in a tool's label.
pub fn height_text(height: Option<f64>) -> String {
    // Three decimals with no "-0.000", as every figure the vertex tools
    // print (everyday's fixed).
    match height {
        Some(height) => format!("z {}", fixed(height, 3)),
        None => String::from("no height"),
    }
}
