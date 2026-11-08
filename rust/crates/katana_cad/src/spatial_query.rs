//! Narrowing a query to the entities that could possibly answer it (PLAN.MD
//! Phase 18; include/katana/cad/spatial_query.hpp, whose functions are in the
//! C++'s `detail` namespace: an implementation decision of the broad phase,
//! public only because the viewport draws through the same narrowing).
//!
//! Snapping, picking and box selection all begin the same way: take a reach
//! box around the cursor or the selection window and consider only the
//! entities whose extent meets it. Before Phase 18 that was a scan of every
//! entity; with an index it is a lookup of the few cells the reach covers.
//!
//! Both paths live behind this one function so there is exactly one
//! definition of "which entities are candidates", and one definition of the
//! box that decides it - see query_extents below, which is the subtle part.

use katana_entity::annotation::DEFAULT_ANNOTATION_SCALE;
use katana_entity::entity_geometry::bounding_box;
use katana_entity::{Entity, Geometry, Model};
use katana_geometry::{Box2, CurveSegment, SpatialId, SpatialIndex};
use katana_math::cpp;
use katana_math::numerics::TWO_PI;

use crate::dimension_draw::{build_dimension, resolve_dimension_style};

/// The box the spatial index stores for an entity, and the box the broad phase
/// tests against.
///
/// It is NOT always the geometric bounding box. Snapping an arc offers its
/// CENTRE, and the centre of an arc lies outside the arc's own bounding box -
/// for a shallow arc, far outside it. Indexing the plain bounding box would
/// therefore make the broad phase reject an arc whose centre is directly under
/// the cursor, and centre snap on arcs would quietly stop working at exactly
/// the moment the index was switched on.
///
/// So the indexed box is the widest extent any query needs: the full circle
/// for an arc, the bounding box for everything else. It stays a BROAD phase
/// for every caller, because each still applies its own exact test - picking
/// tests the true bounding box and so does not become clickable at an arc's
/// centre.
pub fn query_extents(model: &Model, entity: &Entity) -> Box2 {
    match &entity.geometry {
        Geometry::Arc(arc) => return arc.circle().bounding_box(),
        // The same for the drawing system's kinds: each arc segment of a curve
        // polyline offers its centre, and an elliptical arc offers the
        // ellipse's.
        Geometry::CurvePolyline(curved) => {
            let mut extent = curved.bounding_box();
            for i in 0..curved.segment_count() {
                if let CurveSegment::Arc(segment_arc) = curved.segment(i) {
                    extent.expand_box(segment_arc.circle().bounding_box());
                }
            }
            return extent;
        }
        Geometry::Ellipse(ellipse) => {
            let mut whole = *ellipse;
            whole.sweep = TWO_PI;
            return whole.bounding_box();
        }
        Geometry::Dimension(dimension) => {
            // A dimension DRAWS far outside what it measures: the label, the
            // arrows and the extension overshoot all sit beyond the measured
            // points, and how far depends on its style. Culling against the
            // plain bounding box drops a dimension whose label is still on
            // screen.
            //
            // build_dimension is called here rather than a margin being
            // guessed at, because a style with a large text height makes any
            // fixed guess wrong. It costs a handful of vector appends per
            // dimension, and only on the scan path - the indexed path stores
            // this box once when the entity changes.
            let drawing = build_dimension(
                dimension,
                resolve_dimension_style(model, entity),
                DEFAULT_ANNOTATION_SCALE,
            );
            if !drawing.extent.empty() {
                return drawing.extent;
            }
        }
        _ => {}
    }
    bounding_box(&entity.geometry)
}

/// Whether the index is worth using for a query this wide, or whether the
/// plain ordered scan is cheaper.
///
/// It is NOT free to ask an index for everything. A query covering the whole
/// drawing gathers every id, sorts them, and then looks each one up again,
/// against a single ordered walk of the entities. Measured in Release at
/// 100 000 entities, by the share of the drawing's AREA on screen:
///
/// ```text
///     area on screen    visible   scan      indexed
///        1 %              900     4060 us     170 us     index 24x better
///        9 %            8 995     4029 us    2586 us     index 1.6x better
///       25 %           25 195     4881 us    4147 us     index 1.2x better
///       49 %           49 000     4426 us    6173 us     SCAN 1.4x better
///      100 %          100 000     4018 us    8068 us     SCAN 2.0x better
/// ```
///
/// The crossover is around 35% of the area, which is the threshold below. A
/// zoomed-out repaint is exactly the case that would otherwise get slower, and
/// getting slower is not an acceptable price for getting faster elsewhere.
///
/// SpatialIndex::bounds() is never shrunk when an entity is removed (that
/// would make every delete O(n)), so it can be larger than the data really
/// is. That biases this towards the index, by a margin a rebuild removes.
pub fn worth_indexing(index: &SpatialIndex, reach: Box2) -> bool {
    let extent = index.bounds();
    if extent.empty() || reach.empty() {
        return !extent.empty(); // nothing indexed: the scan is trivially cheap
    }
    let total = extent.width() * extent.height();
    if !(total > 0.0) {
        return true; // a degenerate extent says nothing; the index cannot be worse
    }
    let overlap_width = cpp::min(extent.max.x, reach.max.x) - cpp::max(extent.min.x, reach.min.x);
    let overlap_height = cpp::min(extent.max.y, reach.max.y) - cpp::max(extent.min.y, reach.min.y);
    if overlap_width <= 0.0 || overlap_height <= 0.0 {
        return true; // the query is off the data entirely; the index says so at once
    }
    const SCAN_ABOVE_AREA_SHARE: f64 = 0.35;
    (overlap_width * overlap_height) / total < SCAN_ABOVE_AREA_SHARE
}

/// Calls visit(entity) for every entity whose query_extents meets `reach`, in
/// ascending id order.
///
/// `index` may be None, in which case every entity is scanned. The two paths
/// visit exactly the same entities in the same order - asserted by tests that
/// run each query both ways and compare. `scratch` is the caller's reusable
/// buffer, so a mouse-move path allocates nothing (PLAN.MD section 33).
pub fn for_each_candidate(
    model: &Model,
    index: Option<&SpatialIndex>,
    reach: Box2,
    scratch: &mut Vec<SpatialId>,
    mut visit: impl FnMut(&Entity),
) {
    let Some(index) = index.filter(|index| worth_indexing(index, reach)) else {
        model.entities.for_each(|entity| {
            if query_extents(model, entity).intersects(reach) {
                visit(entity);
            }
        });
        return;
    };

    index.query_into(reach, scratch);
    // The index reports ids ascending and find() is a lookup rather than a
    // scan, so the visit order matches the scanning path exactly. Callers rely
    // on that: picking resolves a tie by preferring the entity visited later.
    for &id in scratch.iter() {
        // The index has outlived an entity. That is a defect in whatever
        // maintains it, but a stale id must not be dereferenced, and skipping
        // it degrades to a narrower answer rather than crashing.
        let Some(entity) = model.entities.find(id) else {
            continue;
        };
        if query_extents(model, entity).intersects(reach) {
            visit(entity);
        }
    }
}
