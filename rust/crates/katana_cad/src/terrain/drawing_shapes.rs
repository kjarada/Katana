//! The drawing's polygons and lines as the verbs that edit or measure a
//! surface by them read them from their words (include/katana/cad/terrain/
//! drawing_shapes.hpp, src/katana_cad/terrain/drawing_shapes.cpp): SURFACE
//! EDIT's <polygon> and <line> (surface_edit_verbs.rs, docs/surface_editing.md)
//! and the VOLUME verbs' INSIDE <polygon> (docs/volumes.md).
//!
//! ```text
//! <polygon> | <line> := #id [#id ...] | x,y[,z] x,y[,z] ... | FROM <scope> [WHERE k=v ...]
//! ```
//!
//! The two families each had a reader of their own, written side by side,
//! and they had come to disagree: one took a full ellipse and a polyline
//! drawn back to its start and the other did not, one kept the outline's
//! heights and the other dropped them, one took FROM with no scope word as
//! the selection and the other refused it. This is the one reader, and where
//! they differed its rule is the one that takes more (docs/volumes.md,
//! "Inside a polygon").
//!
//! WHAT AN ENTITY GIVES. As a polygon, closed, its corners never repeating
//! the first at the end:
//!
//! * a closed polyline, or an open one whose last vertex is its first (within
//!   tolerance::GEOMETRIC), drawn back to its start: its vertices, each at the
//!   height its elevation or elevations property gives it (heights_of, the one
//!   reader of them);
//! * a closed curve polyline, or an open one back at its start: its straight
//!   segments exact and its arcs chorded within CURVE_CHORD_TOLERANCE (1 mm
//!   sagitta), a vertex at its own height and a chord point at the height
//!   interpolated along its arc, none where an end has none
//!   (CurvePolyline2::tessellate_with_heights);
//! * a circle, and a full ellipse: chorded within CURVE_CHORD_TOLERANCE, every
//!   corner at the entity's elevation property when it has one;
//! * a closed spline: one whose end returns to its start within GEOMETRIC
//!   (Spline2::is_closed_shape; Spline2 has no closed flag of its own),
//!   flattened as the ellipse is, by Spline2::tessellate within
//!   CURVE_CHORD_TOLERANCE, so no chord's middle is more than 1 mm off the
//!   curve; every corner at the entity's elevation property when it has one,
//!   as the circle's are - a spline holds no height of its own;
//! * none for fewer than three corners left (TooFew); for a line, an arc, an
//!   elliptical arc or another open polyline, which bound nothing (Open); and
//!   for a point, text, an open spline, a dimension ..., which are no outline
//!   at all (NoOutline). A spline that does not return to its start is no
//!   outline rather than an open one: it is not a line that was meant to
//!   close.
//!
//! As a line, its vertices in order, closed when the entity is (and then
//! without the first repeated at the end):
//!
//! * a line; a polyline; a curve polyline, chorded as above; with the heights
//!   as above;
//! * none for fewer than two vertices (TooFew), and for anything else
//!   (NoOutline).
//!
//! TYPED CORNERS are x,y or x,y,z, each a finite number, the third the
//! corner's height. Typed as a polygon they are closed, the last dropped when
//! it repeats the first, and need three corners; as a line, two.
//!
//! TAGGED HOLES. A polygon the drawing has is one closed ring, and a ring
//! inside another is an area of its own. Only a ring that says it is a hole
//! (katana_entity::ring_tags: `source.ring` or `gis.ring` saying `hole`) is
//! one, and with ShapeOptions::join_tagged_holes resolve_shapes takes such
//! rings out of the polygons it reads and gives each to the polygon it lies in
//! as DrawingShape::holes - by the one rule of the feature-table conversion,
//! which uses the same function: the exterior whose part it shares, else the
//! smallest that holds all of it, else the one made nearest before it. A
//! tagged ring that no polygon read in the same call holds stays a polygon of
//! its own and is counted (DrawingShapes::holes_alone), as the conversion
//! counts it. Off by default: SURFACE EDIT's polygons are rings, and a hole
//! there is not asked for.
//!
//! FROM <scope> is the one scope grammar (scope_verbs.rs), resolved by the one
//! matcher; FROM with no scope word after it is the selection, as the grammar
//! says of no scope word. What a scope takes that is no shape is counted by
//! why, never an error here: a scope that takes nothing is reported (CLAUDE.md,
//! section 1.1), and what that means is the verb's.
//!
//! NOTHING HERE IS WORDED FOR A REPLY. The reader gives the data, the counts
//! and why a #id or the typed corners are no shape; each verb family writes
//! its own records and refusals from them, as its tests and its users know
//! them. The one exception is the list of kinds a refusal names
//! (POLYGON_KINDS), kept here so that it changes with the rule.

use katana_core::Result;
use katana_core::text::{equals_ignoring_case, parse_finite_double, parse_integer, trimmed};
use katana_entity::entity::heights_of;
use katana_entity::ring_tags::{ExteriorCandidate, HoleJoiner, is_tagged_hole, ring_part};
use katana_entity::{Entity, EntityId, Geometry, Model};
use katana_geometry::chording::chord_circle;
use katana_geometry::curves2d::CURVE_CHORD_TOLERANCE;
use katana_geometry::{CurvePolyline2, Point2, Polyline2};
use katana_math::numerics::tolerance;

use crate::document::Document;
use crate::scope_verbs::{
    ScopeMatch, ScopeViewProvider, ScopeWords, match_scope, parse_scope_words,
};

/// The kinds a polygon is made from, as a refusal names them, so the sentence
/// and the rule above are changed together.
pub const POLYGON_KINDS: &str =
    "a closed polyline, curve polyline or spline, a circle or an ellipse";
/// The kinds a line is made from, as a refusal names them.
pub const LINE_KINDS: &str = "a line, polyline or curve polyline";

// ---- typed corners ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TypedCorner {
    pub at: Point2,
    pub z: Option<f64>,
}

/// "x,y" or "x,y,z", each part a finite number (blanks around a part
/// allowed); None for anything else, so a word that is not a corner ends a
/// run of them.
pub fn parse_typed_corner(word: &str) -> Option<TypedCorner> {
    let mut parts = Vec::new();
    for part in word.split(',') {
        parts.push(parse_finite_double(trimmed(part.as_bytes()))?);
    }
    if parts.len() != 2 && parts.len() != 3 {
        return None;
    }
    Some(TypedCorner {
        at: Point2::new(parts[0], parts[1]),
        z: parts.get(2).copied(),
    })
}

// ---- one shape ---------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ShapeKind {
    Polygon,
    Line,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DrawingShape {
    /// The entity it is; None for typed corners.
    pub entity: Option<EntityId>,
    /// Its corners or vertices in order; a polygon's closed and never
    /// repeating its first corner at the end.
    pub outline: Polyline2,
    /// One per vertex of `outline`; None where a vertex has none.
    pub heights: Vec<Option<f64>>,
    /// An arc, a circle, an ellipse or a spline was chorded to make it.
    pub chorded: bool,
    /// A polygon read with ShapeOptions::join_tagged_holes: the rings tagged
    /// as holes that lie in it, each closed and never repeating its first
    /// corner, and the entities they are. Empty otherwise, and for a line.
    pub holes: Vec<Polyline2>,
    pub hole_entities: Vec<EntityId>,
}

/// Why an entity, or the typed corners, are no shape of the kind asked for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum NotAShape {
    /// A #id the drawing has no entity of.
    NoEntity,
    /// A polygon asked: a line, an arc, an open polyline bounds nothing.
    Open,
    /// Under three corners for a polygon, two vertices for a line.
    TooFew,
    /// None of the kinds the rule takes.
    #[default]
    NoOutline,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShapeOf {
    pub shape: Option<DrawingShape>,
    /// Why there is none; meaningless when there is one.
    pub why: NotAShape,
    /// TooFew: the corners or vertices left once a polygon's closing repeat of
    /// its first corner was taken off - the count a refusal can honestly
    /// quote, where the words typed counted that repeat.
    pub corners: usize,
}

fn none(why: NotAShape, corners: usize) -> ShapeOf {
    ShapeOf {
        shape: None,
        why,
        corners,
    }
}

fn some(shape: DrawingShape) -> ShapeOf {
    ShapeOf {
        shape: Some(shape),
        ..ShapeOf::default()
    }
}

fn back_at_its_start(vertices: &[Point2]) -> bool {
    vertices.len() > 1
        && vertices[0].distance_to(vertices[vertices.len() - 1]) <= tolerance::GEOMETRIC
}

// A closed outline names each corner once: the first repeated at the end - as
// a ring drawn back to its start, and every closed curve's walk, give it - is
// taken off with its height, as often as it is there.
fn drop_closing_repeat(shape: &mut DrawingShape) {
    while back_at_its_start(&shape.outline.vertices) {
        shape.outline.vertices.pop();
        shape.heights.pop();
    }
}

fn as_polygon(mut shape: DrawingShape) -> ShapeOf {
    shape.outline.closed = true;
    drop_closing_repeat(&mut shape);
    if shape.outline.vertices.len() < 3 {
        return none(NotAShape::TooFew, shape.outline.vertices.len());
    }
    some(shape)
}

fn as_line(mut shape: DrawingShape) -> ShapeOf {
    if shape.outline.closed {
        drop_closing_repeat(&mut shape);
    }
    if shape.outline.vertices.len() < 2 {
        return none(NotAShape::TooFew, shape.outline.vertices.len());
    }
    some(shape)
}

fn vertex_shape(polyline: &Polyline2, entity: &Entity) -> DrawingShape {
    DrawingShape {
        outline: polyline.clone(),
        heights: heights_of(&entity.properties, polyline.vertices.len()),
        ..DrawingShape::default()
    }
}

// The walk of a curve polyline, its arcs chorded and every point at its
// height: a vertex's own, a chord point's interpolated along its arc. A closed
// one's walk ends on its first point again.
fn walk_of(curve: &CurvePolyline2) -> DrawingShape {
    let mut shape = DrawingShape::default();
    for point in curve.tessellate_with_heights(CURVE_CHORD_TOLERANCE) {
        shape.outline.vertices.push(point.position);
        shape.heights.push(point.height);
    }
    shape.outline.closed = curve.closed;
    shape.chorded = curve.has_arcs();
    shape
}

// A circle's or an ellipse's chords, every corner at the entity's one
// elevation: an elevations list would have to have a value per chord point,
// which no list written for the entity has (heights_of passes it over).
fn chord_shape(ring: Vec<Point2>, entity: &Entity) -> DrawingShape {
    let heights = heights_of(&entity.properties, ring.len());
    DrawingShape {
        outline: Polyline2::new(ring, true),
        heights,
        chorded: true,
        ..DrawingShape::default()
    }
}

fn polygon_of(entity: &Entity) -> ShapeOf {
    match &entity.geometry {
        Geometry::Polyline(polyline) => {
            if !polyline.closed && !back_at_its_start(&polyline.vertices) {
                return none(NotAShape::Open, 0);
            }
            as_polygon(vertex_shape(polyline, entity))
        }
        Geometry::CurvePolyline(curve) => {
            let walk = walk_of(curve);
            if !curve.closed && !back_at_its_start(&walk.outline.vertices) {
                return none(NotAShape::Open, 0);
            }
            as_polygon(walk)
        }
        Geometry::Circle(circle) => as_polygon(chord_shape(
            chord_circle(circle, CURVE_CHORD_TOLERANCE),
            entity,
        )),
        Geometry::Ellipse(ellipse) => {
            if !ellipse.is_full() {
                return none(NotAShape::Open, 0);
            }
            as_polygon(chord_shape(
                ellipse.tessellate(CURVE_CHORD_TOLERANCE),
                entity,
            ))
        }
        Geometry::Spline(spline) => {
            // A spline has no closed flag; one whose end returns to its start
            // is a closed outline, and its walk ends on its first point again,
            // which as_polygon takes off. Not closed, it is no outline at all
            // (it is not a line that was meant to bound anything).
            if !spline.is_closed_shape() {
                return none(NotAShape::NoOutline, 0);
            }
            as_polygon(chord_shape(
                spline.tessellate(CURVE_CHORD_TOLERANCE),
                entity,
            ))
        }
        Geometry::Line(_) | Geometry::Arc(_) => none(NotAShape::Open, 0),
        _ => none(NotAShape::NoOutline, 0),
    }
}

fn line_of(entity: &Entity) -> ShapeOf {
    match &entity.geometry {
        Geometry::Line(segment) => as_line(vertex_shape(
            &Polyline2::new(vec![segment.start, segment.end], false),
            entity,
        )),
        Geometry::Polyline(polyline) => as_line(vertex_shape(polyline, entity)),
        Geometry::CurvePolyline(curve) => as_line(walk_of(curve)),
        _ => none(NotAShape::NoOutline, 0),
    }
}

/// The shape `entity` is as `kind`, by the rules of the module comment, its
/// `entity` set.
pub fn shape_of(entity: &Entity, kind: ShapeKind) -> ShapeOf {
    let mut of = if kind == ShapeKind::Polygon {
        polygon_of(entity)
    } else {
        line_of(entity)
    };
    if let Some(shape) = &mut of.shape {
        shape.entity = Some(entity.id);
    }
    of
}

/// The shape typed corners make as `kind` (the C++ overload
/// `shapeOf(const std::vector<TypedCorner>&, ShapeKind)`).
pub fn shape_of_corners(corners: &[TypedCorner], kind: ShapeKind) -> ShapeOf {
    let mut shape = DrawingShape::default();
    for corner in corners {
        shape.outline.vertices.push(corner.at);
        shape.heights.push(corner.z);
    }
    if kind == ShapeKind::Polygon {
        as_polygon(shape)
    } else {
        as_line(shape)
    }
}

// ---- the words ---------------------------------------------------------------------------------

/// What the words of a <polygon> or <line> name, read and nothing looked up.
/// At most one of ids, corners and scope is given; none when the word at `at`
/// is neither #id, FROM nor a corner, or there is no word.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShapeWords {
    pub ids: Vec<EntityId>,
    pub corners: Vec<TypedCorner>,
    pub scope: Option<ScopeWords>,
    /// A word after # that is not # and a whole number from 1: the reading
    /// stopped on it, and the verb refuses it in its own words.
    pub bad_id: Option<String>,
}

impl ShapeWords {
    pub fn empty(&self) -> bool {
        self.ids.is_empty()
            && self.corners.is_empty()
            && self.scope.is_none()
            && self.bad_id.is_none()
    }
}

/// Reads a <polygon> or <line> from `words` at `at`, leaving `at` after it:
/// one or more #ids, typed corners, or FROM and a scope. Fails only as
/// parse_scope_words fails, with its refusal.
pub fn read_shape_words(words: &[String], at: &mut usize) -> Result<ShapeWords> {
    let mut read = ShapeWords::default();
    if *at >= words.len() {
        return Ok(read);
    }
    if equals_ignoring_case(words[*at].as_bytes(), b"FROM") {
        *at += 1;
        read.scope = Some(parse_scope_words(words, at)?);
        return Ok(read);
    }
    if words[*at].starts_with('#') {
        while *at < words.len() && words[*at].starts_with('#') {
            let id = parse_integer(&words[*at].as_bytes()[1..]);
            let Some(id) = id.filter(|id| *id > 0) else {
                read.bad_id = Some(words[*at].clone());
                return Ok(read);
            };
            read.ids.push(id as EntityId);
            *at += 1;
        }
        return Ok(read);
    }
    while *at < words.len() {
        let Some(corner) = parse_typed_corner(&words[*at]) else {
            break;
        };
        read.corners.push(corner);
        *at += 1;
    }
    Ok(read)
}

// ---- what the words name in the drawing ---------------------------------------------------------

/// The entities `words` name: their #ids as given, or what their scope
/// matched (ascending, match_scope's order).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NamedEntities {
    pub ids: Vec<EntityId>,
    /// FROM: what the scope took.
    pub scope: Option<ScopeMatch>,
    /// #ids: the first the drawing has no entity of, where the naming
    /// stopped; `ids` are those before it.
    pub missing: Option<EntityId>,
}

/// Fails only as match_scope fails (VIEW headless, a layer the drawing lacks).
/// Typed corners name no entity.
pub fn entities_named(
    document: &Document,
    words: &ShapeWords,
    views: ScopeViewProvider<'_>,
) -> Result<NamedEntities> {
    let mut named = NamedEntities::default();
    if let Some(scope) = &words.scope {
        let found = match_scope(document, scope, views)?;
        named.ids.clone_from(&found.matched);
        named.scope = Some(found);
        return Ok(named);
    }
    for &id in &words.ids {
        if document.model().entities.find(id).is_none() {
            named.missing = Some(id);
            return Ok(named);
        }
        named.ids.push(id);
    }
    Ok(named)
}

/// What a scope took that is no shape, by why.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SkippedShapes {
    pub open: usize,
    pub too_few: usize,
    pub no_outline: usize,
}

impl SkippedShapes {
    pub fn total(&self) -> usize {
        self.open + self.too_few + self.no_outline
    }
}

/// A #id, or the typed corners, that is no shape, which refuses the line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ShapeRefusal {
    /// The #id; None for the typed corners.
    pub entity: Option<EntityId>,
    pub why: NotAShape,
    /// TooFew: the distinct corners the typed words made (ShapeOf::corners).
    pub corners: usize,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DrawingShapes {
    /// In the order named: the #ids as typed, a scope's ascending.
    pub shapes: Vec<DrawingShape>,
    /// FROM: what the scope took, and what it took that is no shape. Taking
    /// no shape is no refusal here.
    pub scope: Option<ScopeMatch>,
    pub skipped: SkippedShapes,
    /// #ids and typed corners: the first that is no shape, where the reading
    /// stopped. A #id must be one, so the verb refuses the line.
    pub refused: Option<ShapeRefusal>,
    /// ShapeOptions::join_tagged_holes: the tagged rings taken into the
    /// polygons they lie in (no longer in `shapes`), and those that lie in
    /// none of the polygons read and are left in `shapes` as polygons of their
    /// own.
    pub holes_joined: usize,
    pub holes_alone: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ShapeOptions {
    /// See "TAGGED HOLES" in the module comment. Polygons only; a line has no
    /// hole.
    pub join_tagged_holes: bool,
}

fn entity_of<'a>(model: &'a Model, shape: &DrawingShape) -> Option<&'a Entity> {
    shape.entity.and_then(|id| model.entities.find(id))
}

// Gives each tagged ring among `taken.shapes` to the polygon it lies in, by
// the rule of katana_entity::ring_tags, and takes it out of the shapes; a ring
// that no polygon of the call holds is left where it is and counted.
fn join_tagged_holes(document: &Document, taken: &mut DrawingShapes) {
    let model = document.model();
    let mut exteriors: Vec<usize> = Vec::new(); // index in taken.shapes
    let mut candidates: Vec<ExteriorCandidate> = Vec::new();
    for (i, shape) in taken.shapes.iter().enumerate() {
        let Some(entity) = entity_of(model, shape) else {
            continue; // typed corners are no hole's exterior
        };
        if is_tagged_hole(entity) {
            continue; // a hole is not one
        }
        exteriors.push(i);
        candidates.push(ExteriorCandidate {
            id: entity.id,
            part: ring_part(entity),
            ring: shape.outline.clone(),
        });
    }
    let joiner = HoleJoiner::new(candidates);
    // Each hole's exterior is chosen first and given to it after, in the
    // order of the holes: a hole is never an exterior, so no choice reads a
    // ring that an earlier hole changed, and the holes go into each exterior
    // in the order the C++ pushes them.
    let mut joins: Vec<(usize, usize, EntityId)> = Vec::new(); // hole, exterior, the hole's id
    for (i, shape) in taken.shapes.iter().enumerate() {
        let Some(entity) = entity_of(model, shape) else {
            continue;
        };
        if !is_tagged_hole(entity) {
            continue;
        }
        match joiner.exterior_of(&shape.outline.vertices, entity.id, &ring_part(entity)) {
            Some(best) => joins.push((i, exteriors[best], entity.id)),
            None => taken.holes_alone += 1,
        }
    }
    let mut joined = vec![false; taken.shapes.len()];
    for (hole, exterior, id) in joins {
        let ring = taken.shapes[hole].outline.clone();
        let target = &mut taken.shapes[exterior];
        target.holes.push(ring);
        target.hole_entities.push(id);
        joined[hole] = true;
        taken.holes_joined += 1;
    }
    let shapes = std::mem::take(&mut taken.shapes);
    taken.shapes = shapes
        .into_iter()
        .zip(joined)
        .filter(|(_, joined)| !joined)
        .map(|(shape, _)| shape)
        .collect();
}

/// The shapes `words` name as `kind`: each #id in turn, the typed corners as
/// one shape, or what the scope takes. Words that name nothing (empty()) are
/// typed corners of none: refused, TooFew. A bad_id is the verb's to refuse
/// before this is asked. Fails only as entities_named fails. (The C++ defaults
/// `options` to ShapeOptions{}.)
pub fn resolve_shapes(
    document: &Document,
    words: &ShapeWords,
    kind: ShapeKind,
    views: ScopeViewProvider<'_>,
    options: &ShapeOptions,
) -> Result<DrawingShapes> {
    let mut taken = DrawingShapes::default();
    if words.scope.is_none() && words.ids.is_empty() {
        let typed = shape_of_corners(&words.corners, kind);
        match typed.shape {
            Some(shape) => taken.shapes.push(shape),
            None => {
                taken.refused = Some(ShapeRefusal {
                    entity: None,
                    why: typed.why,
                    corners: typed.corners,
                });
            }
        }
        return Ok(taken);
    }
    let named = entities_named(document, words, views)?;
    if let Some(missing) = named.missing {
        taken.refused = Some(ShapeRefusal {
            entity: Some(missing),
            why: NotAShape::NoEntity,
            corners: 0,
        });
        return Ok(taken);
    }
    taken.scope = named.scope;
    for &id in &named.ids {
        let Some(entity) = document.model().entities.find(id) else {
            continue; // a scope matches what the drawing has; checked above for #ids
        };
        let of = shape_of(entity, kind);
        if let Some(shape) = of.shape {
            taken.shapes.push(shape);
        } else if taken.scope.is_none() {
            taken.refused = Some(ShapeRefusal {
                entity: Some(id),
                why: of.why,
                corners: 0,
            });
            return Ok(taken);
        } else if of.why == NotAShape::Open {
            taken.skipped.open += 1;
        } else if of.why == NotAShape::TooFew {
            taken.skipped.too_few += 1;
        } else {
            taken.skipped.no_outline += 1;
        }
    }
    if options.join_tagged_holes && kind == ShapeKind::Polygon {
        join_tagged_holes(document, &mut taken);
    }
    Ok(taken)
}
