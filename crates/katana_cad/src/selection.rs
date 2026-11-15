//! Selection set, selection filters and picking (PLAN.MD Phases 08/09;
//! include/katana/cad/selection.hpp).

use std::collections::BTreeSet;

use katana_entity::curve_pieces::curve_pieces;
use katana_entity::entity_geometry::{bounding_box, distance_to};
use katana_entity::tables::ResolvedLayer;
use katana_entity::{Entity, EntityDatabase, EntityId, EntityType, Geometry, Model};
use katana_geometry::curves2d::CURVE_CHORD_TOLERANCE;
use katana_geometry::intersection::{Intersect, intersect};
use katana_geometry::polygon::clip;
use katana_geometry::{Box2, Curve2, Point2, Polyline2, Segment2, SpatialIndex};

use crate::drawing::construction::is_construction_layer;
use crate::layer_overrides::{LayerOverrides, NO_LAYER_OVERRIDES};
use crate::spatial_query::for_each_candidate;

/// Restricts which entities picking may return. Default: everything.
#[derive(Clone, Debug, Default)]
pub struct SelectionFilter<'a> {
    /// Empty: all types.
    pub types: BTreeSet<EntityType>,
    /// None: all layers.
    pub layer: Option<String>,
    /// The view the pick is made in; None for the document rule alone. A layer
    /// hidden in that view cannot be picked there, or Delete would erase
    /// something the user cannot see (see LayerOverrides).
    pub view: Option<&'a LayerOverrides>,
}

impl SelectionFilter<'_> {
    pub fn accepts(&self, entity: &Entity) -> bool {
        if !self.types.is_empty() && !self.types.contains(&entity.r#type()) {
            return false;
        }
        self.layer
            .as_ref()
            .is_none_or(|layer| entity.layer == *layer)
    }
}

/// Ordered (ascending id), duplicate free.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SelectionSet {
    ids: BTreeSet<EntityId>,
}

impl SelectionSet {
    pub fn set(&mut self, ids: Vec<EntityId>) {
        self.ids = ids.into_iter().collect();
    }

    pub fn add(&mut self, id: EntityId) {
        self.ids.insert(id);
    }

    pub fn remove(&mut self, id: EntityId) {
        self.ids.remove(&id);
    }

    pub fn toggle(&mut self, id: EntityId) {
        if !self.ids.remove(&id) {
            self.ids.insert(id);
        }
    }

    pub fn clear(&mut self) {
        self.ids.clear();
    }

    pub fn contains(&self, id: EntityId) -> bool {
        self.ids.contains(&id)
    }

    pub fn empty(&self) -> bool {
        self.ids.is_empty()
    }

    pub fn size(&self) -> usize {
        self.ids.len()
    }

    pub fn ids(&self) -> Vec<EntityId> {
        self.ids.iter().copied().collect()
    }

    /// Drops ids that are no longer in the model. Returns true if anything
    /// changed.
    pub fn prune(&mut self, entities: &EntityDatabase) -> bool {
        let before = self.ids.len();
        self.ids.retain(|id| entities.contains(*id));
        self.ids.len() != before
    }
}

// THE visibility rule, and the one every view and every picker asks, so that
// what the plan view hides the 3D view, a section, a surface built from the
// drawing, snapping and selection all hide too (audit MOD-01, REN-03, REN-04).
// A layer counts as hidden or locked when an ANCESTOR is (PLAN.MD 5.1): turning
// off "design" turns off "design/surface/tin1".
//
// `view` is the second half: layers hidden in ONE view (LayerOverrides). It has
// no default on purpose, so that every caller states whether it is asking for a
// view or for the document - pass NO_LAYER_OVERRIDES for the latter. A default
// would let a new consumer silently ignore the view it is drawn in.

/// Visible entity on a layer that is shown, unlocked and not hidden in `view`.
pub fn is_selectable(model: &Model, entity: &Entity, view: &LayerOverrides) -> bool {
    // The drawn test once, here, rather than repeated inline: it used to be
    // written out a second time and the two could drift.
    let layer = model.layers.resolve(&entity.layer);
    is_drawn_resolved(&layer, entity, view) && !layer.locked
}

/// Visible entity on a layer that is shown and not hidden in `view` (a locked
/// layer still draws).
pub fn is_drawn(model: &Model, entity: &Entity, view: &LayerOverrides) -> bool {
    is_drawn_resolved(&model.layers.resolve(&entity.layer), entity, view)
}

/// The same rule for a caller that has already resolved the layer (the C++
/// `isDrawn(const ResolvedLayer&, ...)` overload). The viewport draws tens of
/// thousands of entities a frame and needs the layer for its colour too, so
/// it resolves once and asks this.
pub fn is_drawn_resolved(
    layer: &ResolvedLayer<'_>,
    entity: &Entity,
    view: &LayerOverrides,
) -> bool {
    entity.visible && is_layer_drawn(layer, &entity.layer, view)
}

/// The layer's half of that rule, for something drawn on a layer that is not
/// an entity (a road object, docs/road_objects.md): the document shows the
/// layer (it and every ancestor are shown, and a layer that is not there is
/// not) and `view` does not hide the path. is_drawn is this and the entity's
/// own switch.
pub fn is_layer_drawn(layer: &ResolvedLayer<'_>, path: &str, view: &LayerOverrides) -> bool {
    layer.shown && !view.hides(path)
}

/// The ghost rule, the drawn rule's other half: `view` alone hides the entity -
/// the document draws it and a layer the view hides of its own takes it out.
/// Where a view shows a SELECTED entity faintly instead of not at all
/// (selection_style.rs; docs/desktop.md, "The selection in every view"): the
/// plan painter, the 3D scene and a section ask this one rule, so none of
/// them ghosts what the document hides - a layer switched off in the drawing,
/// an entity made invisible - which stays hidden in every view.
pub fn is_ghost(model: &Model, entity: &Entity, view: &LayerOverrides) -> bool {
    // The view's test first: it answers at once for a view that hides
    // nothing of its own, which is most views, before the layer is resolved.
    view.hides(&entity.layer) && is_drawn(model, entity, &NO_LAYER_OVERRIDES)
}

/// The box round everything `view` draws: each entity passing the rule, by its
/// true geometry (entity's bounding_box - an arc's own extent, not its
/// circle's). Empty when nothing is drawn. What a view's Zoom Extents frames,
/// so a view zooms to what it shows rather than to a stray on a layer it
/// hides: the point audit REN-03 made for the 3D view, made here for every
/// view.
pub fn drawn_extent(model: &Model, view: &LayerOverrides) -> Box2 {
    let mut extent = Box2::default();
    model.entities.for_each(|entity| {
        // A construction line reaches 100 km each way; it is an aid to the
        // drawing, not part of its extent (drawing/construction.rs).
        if is_drawn(model, entity, view) && !is_construction_layer(&entity.layer) {
            extent.expand_box(bounding_box(&entity.geometry));
        }
    });
    extent
}

/// The box round the entities `ids` names, measured as drawn_extent measures
/// one: what ZOOM frames for a scope (view_verbs), and what the window frames
/// for a selection (Select by ID, a style's users) - one measure, so the three
/// cannot frame the same entities differently. Ids that name no entity are
/// passed over, and so are construction lines, which drawn_extent leaves out
/// for the same reason; empty when nothing is left.
pub fn extent_of(model: &Model, ids: &[EntityId]) -> Box2 {
    let mut extent = Box2::default();
    for &id in ids {
        if let Some(entity) = model.entities.find(id)
            && !is_construction_layer(&entity.layer)
        {
            extent.expand_box(bounding_box(&entity.geometry));
        }
    }
    extent
}

fn view_of<'a>(filter: &SelectionFilter<'a>) -> &'a LayerOverrides {
    filter.view.unwrap_or(&NO_LAYER_OVERRIDES)
}

/// Nearest selectable entity whose geometry lies within `tolerance` (model
/// units) of `point`. Ties go to the higher id: the entity drawn last, i.e. on
/// top. `index`, when supplied, narrows the search instead of scanning the
/// model (PLAN.MD Phase 18). It must be in step with `model`; Document keeps
/// one that is. The answer is identical either way - the index is a broad
/// phase and the exact distance test is unchanged - which is asserted by a
/// test comparing the two paths.
pub fn pick_entity(
    model: &Model,
    point: Point2,
    tolerance: f64,
    filter: &SelectionFilter<'_>,
    index: Option<&SpatialIndex>,
) -> Option<EntityId> {
    let mut best = None;
    let mut best_distance = tolerance;
    let reach = Box2::new(point, point).inflated(tolerance);
    let mut scratch = Vec::new();
    let view = view_of(filter);
    for_each_candidate(model, index, reach, &mut scratch, |entity| {
        // The broad phase uses query_extents, which is wider than the bounding
        // box for an arc so that snapping can reach its centre. Picking must
        // NOT be that generous - clicking an arc's centre should not select it
        // - so the true bounding box is still tested here.
        if !is_selectable(model, entity, view)
            || !filter.accepts(entity)
            || !bounding_box(&entity.geometry).intersects(reach)
        {
            return;
        }
        let distance = distance_to(&entity.geometry, point);
        if distance <= best_distance {
            // <=: later (higher id, drawn on top) wins ties
            best_distance = distance;
            best = Some(entity.id);
        }
    });
    best
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum BoxSelectionMode {
    /// Only entities entirely inside the box.
    #[default]
    Window,
    /// Entities inside or touching the box.
    Crossing,
}

pub fn pick_in_box(
    model: &Model,
    bounds: Box2,
    mode: BoxSelectionMode,
    filter: &SelectionFilter<'_>,
    index: Option<&SpatialIndex>,
) -> Vec<EntityId> {
    let mut picked = Vec::new();
    if bounds.empty() {
        return picked;
    }
    let mut scratch = Vec::new();
    let view = view_of(filter);
    for_each_candidate(model, index, bounds, &mut scratch, |entity| {
        if !is_selectable(model, entity, view) || !filter.accepts(entity) {
            return;
        }
        let extents = bounding_box(&entity.geometry);
        let inside = bounds.contains_box(extents);
        let selected = inside
            || (mode == BoxSelectionMode::Crossing
                && extents.intersects(bounds)
                && touches(&bounds, &entity.geometry));
        if selected {
            picked.push(entity.id);
        }
    });
    picked
}

fn edges_of(bounds: &Box2) -> [Segment2; 4] {
    let a = bounds.min;
    let b = Point2::new(bounds.max.x, bounds.min.y);
    let c = bounds.max;
    let d = Point2::new(bounds.min.x, bounds.max.y);
    [
        Segment2::new(a, b),
        Segment2::new(b, c),
        Segment2::new(c, d),
        Segment2::new(d, a),
    ]
}

fn crosses_an_edge<C>(bounds: &Box2, curve: &C) -> bool
where
    Segment2: Intersect<C>,
{
    edges_of(bounds)
        .iter()
        .any(|edge| intersect(edge, curve).exists())
}

fn polyline_touches(bounds: &Box2, polyline: &Polyline2) -> bool {
    (0..polyline.segment_count()).any(|i| clip(&polyline.segment(i), bounds).is_some())
}

// A curve polyline by its segments and arcs; an ellipse and a spline by their
// chords (entity's curve_pieces.rs), good to a millimetre.
fn any_piece_touches(bounds: &Box2, geometry: &Geometry) -> bool {
    curve_pieces(geometry, CURVE_CHORD_TOLERANCE)
        .iter()
        .any(|piece| match piece {
            Curve2::Segment(segment) => clip(segment, bounds).is_some(),
            Curve2::Arc(arc) => {
                bounds.contains(arc.start_point())
                    || bounds.contains(arc.end_point())
                    || crosses_an_edge(bounds, arc)
            }
            Curve2::Circle(circle) => {
                bounds.contains(circle.point_at_angle(0.0)) || crosses_an_edge(bounds, circle)
            }
        })
}

/// True when the drawn geometry has at least one point inside or on the box.
fn touches(bounds: &Box2, geometry: &Geometry) -> bool {
    match geometry {
        Geometry::Point(point) => bounds.contains(point.position),
        Geometry::Line(segment) => clip(segment, bounds).is_some(),
        Geometry::Polyline(polyline) => polyline_touches(bounds, polyline),
        Geometry::Circle(circle) => {
            if bounds.contains(circle.point_at_angle(0.0)) {
                return true; // some point of the curve is inside
            }
            crosses_an_edge(bounds, circle)
        }
        Geometry::Arc(arc) => {
            bounds.contains(arc.start_point())
                || bounds.contains(arc.end_point())
                || crosses_an_edge(bounds, arc)
        }
        // Text and dimensions are picked by their extents; a label by its
        // anchor (and dragged position), which is all of it the model can
        // place without a scale.
        Geometry::Text(_) | Geometry::Dimension(_) | Geometry::Label(_) => {
            bounding_box(geometry).intersects(*bounds)
        }
        // A leader by its line, as a polyline is.
        Geometry::Leader(leader) => {
            polyline_touches(bounds, &Polyline2::new(leader.vertices.clone(), false))
        }
        Geometry::CurvePolyline(_) | Geometry::Ellipse(_) | Geometry::Spline(_) => {
            any_piece_touches(bounds, geometry)
        }
    }
}

/// Whether `geometry`, as drawn, has at least one point inside or on `bounds`:
/// the test a crossing box selects by (pick_in_box), and what a sheet
/// generator asks of a tile (plotting's draws_in). Text, dimensions and labels
/// count by their boxes; an ellipse and a spline by their chords.
pub fn touches_box(geometry: &Geometry, bounds: &Box2) -> bool {
    !bounds.empty() && bounding_box(geometry).intersects(*bounds) && touches(bounds, geometry)
}
