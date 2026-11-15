//! Editing leaders (include/katana/cad/annotation/leader_edit.hpp;
//! docs/annotation.md, "Smart leaders"): what the LEADER and BALLOON verbs
//! and the window's Leaders manager both do. Each operation checks its edit
//! against the model and returns ONE command - or refuses, saying why in the
//! words the command line replies with - so a verb typed, a button pressed
//! and an agent's call make the same change, and one undo takes it back.
//! Nothing here executes: the caller runs the command through
//! Document::execute, whose associative update then keeps every leader on
//! what it is on.
//!
//! Whatever reads what a leader says takes the drawing's LeaderContext - its
//! surfaces and alignments (leader_presets.rs, leader_context) - so a note
//! reading a surface is checked, previewed and frozen as it is drawn. Left
//! out (`&LeaderContext::default()`, the C++ default argument), a value of a
//! surface or an alignment is absent.
//!
//! The C++ returns a null CommandPtr for "nothing to change"; here that is
//! `Option<CommandPtr>`, since a Rust command cannot be null.

use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::fmt;

use katana_commands::{
    ChangeSet, ChangeSetCommand, CommandPtr, EntityAttributes, remove_entity_property,
    set_entity_property,
};
use katana_core::error::make_error;
use katana_core::text::{equals_ignoring_case, format_exact_real, parse_finite_double};
use katana_core::{Error, ErrorCode, Result, Status};
use katana_entity::anchor::{nearest_anchor, resolve_anchor};
use katana_entity::entity::to_string;
use katana_entity::label_text::{format_value, rename_template_fields, template_fields};
use katana_entity::leader_values::{
    check_leader_says_something, check_leader_template, context_value_name, is_smart,
    is_surface_context_kind, leader_context_kinds, leader_note, leader_value_names, leader_values,
};
use katana_entity::{
    AnchorPoint, AnchorRef, ArrowHead, CalloutShape, Entity, EntityId, Geometry, LabelValues,
    LeaderContext, LeaderGeometry, Model, PropertyValue, validate,
};
use katana_geometry::curves2d::CURVE_CHORD_TOLERANCE;
use katana_geometry::{Containment, Point2, Polyline2, Vec2};
use katana_math::cpp;
use katana_math::numerics::{DEG_TO_RAD, PI, TWO_PI, normalize_angle};

use crate::annotation::dimension_build::AnchoredPoint;
use crate::annotation::leader_draw::code_properties;

/// What a leader's note is (the C++ `LeaderNote::Kind`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum LeaderNoteKind {
    /// Literal text.
    #[default]
    Text,
    /// A template read off the entity its tip is on.
    Template,
    /// A label style's template.
    LabelStyle,
}

/// What a leader's note is: literal text, a template read off the entity its
/// tip is on, or a label style's template (LeaderGeometry::fields and
/// ::label_style).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LeaderNote {
    pub kind: LeaderNoteKind,
    /// The literal note, or the template; for LabelStyle the style's name.
    pub text: String,
}

/// The note `leader` has now.
pub fn note_of(leader: &LeaderGeometry) -> LeaderNote {
    if !leader.label_style.is_empty() {
        return LeaderNote {
            kind: LeaderNoteKind::LabelStyle,
            text: leader.label_style.clone(),
        };
    }
    LeaderNote {
        kind: if leader.fields {
            LeaderNoteKind::Template
        } else {
            LeaderNoteKind::Text
        },
        text: leader.text.clone(),
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LeaderVertexMove {
    pub index: usize,
    pub position: Point2,
}

/// A change to leaders: each member that is set is applied, each unset one
/// left as it is.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LeaderChange {
    pub note: Option<LeaderNote>,
    pub arrow: Option<ArrowHead>,
    pub callout: Option<CalloutShape>,
    /// The text style; empty for the default.
    pub text_style: Option<String>,
    /// Millimetres on paper.
    pub paper_height: Option<f64>,
    pub arrow_size: Option<f64>,
    pub landing: Option<f64>,
    /// Moves the tip, and says what it is on (nothing, for a plain point).
    pub tip: Option<AnchoredPoint>,
    /// Moves the last vertex: where the note hangs.
    pub hang: Option<Point2>,
    pub vertex: Option<LeaderVertexMove>,
    /// An edit whose inner value is absent restores automatic direction.
    pub landing_angle: Option<Option<f64>>,
}

impl LeaderChange {
    pub fn empty(&self) -> bool {
        self.note.is_none()
            && self.arrow.is_none()
            && self.callout.is_none()
            && self.text_style.is_none()
            && self.paper_height.is_none()
            && self.arrow_size.is_none()
            && self.landing.is_none()
            && self.tip.is_none()
            && self.hang.is_none()
            && self.vertex.is_none()
            && self.landing_angle.is_none()
    }
}

// `changes` as one step named `name`.
fn command_of(name: &str, changes: ChangeSet) -> CommandPtr {
    Box::new(ChangeSetCommand::new(
        name,
        move |_context| Ok(changes.clone()),
        false,
    ))
}

fn leader_in(model: &Model, id: EntityId) -> &LeaderGeometry {
    match model.entities.find(id).map(|entity| &entity.geometry) {
        Some(Geometry::Leader(leader)) => leader,
        // Every caller has required_leaders first, as the C++'s std::get
        // assumes.
        _ => panic!("entity {id} is not a leader"),
    }
}

fn leader_of_mut(entity: &mut Entity) -> &mut LeaderGeometry {
    match &mut entity.geometry {
        Geometry::Leader(leader) => leader,
        _ => panic!("entity {} is not a leader", entity.id),
    }
}

// A whole number as std::from_chars reads one into a long long: an optional
// minus and digits, the whole text, in range. Rust's parse also takes a '+',
// which from_chars does not.
fn whole_number(text: &str) -> Option<i64> {
    if text.starts_with('+') {
        return None;
    }
    text.parse::<i64>().ok()
}

// Whether a plain leader's note is a whole number: what a numbered balloon
// says.
fn is_number(text: &str) -> bool {
    whole_number(text).is_some()
}

fn is_numbered_balloon(leader: &LeaderGeometry) -> bool {
    leader.callout == CalloutShape::Circle && !is_smart(leader) && is_number(&leader.text)
}

// In id order, each once.
fn unique(mut ids: Vec<EntityId>) -> Vec<EntityId> {
    ids.sort_unstable();
    ids.dedup();
    ids
}

// Where LEADER FOR puts a tip on `entity`; `angle` is the direction from the
// tip to the note, radians. None for what offers no place (a dimension, a
// label, a leader).
fn natural_anchor(entity: &Entity, angle: f64) -> Option<AnchorRef> {
    let mut reference = AnchorRef {
        entity: entity.id,
        point: AnchorPoint::Along,
        ..AnchorRef::default()
    };
    match &entity.geometry {
        Geometry::Point(_) | Geometry::Text(_) => {
            reference.point = AnchorPoint::Position;
            Some(reference)
        }
        Geometry::Line(_) | Geometry::Arc(_) => {
            reference.parameter = 0.5;
            Some(reference)
        }
        Geometry::Circle(_) => {
            let turn = normalize_angle(angle) / TWO_PI;
            reference.parameter = if turn < 1.0 { turn } else { 0.0 };
            Some(reference)
        }
        Geometry::Polyline(polyline) => {
            let v = &polyline.vertices;
            if polyline.closed && v.len() >= 3 {
                reference.point = AnchorPoint::Inside;
                return Some(reference);
            }
            let half = 0.5 * polyline.length();
            if !(half > 0.0) || v.len() < 2 {
                return None;
            }
            let mut before = 0.0;
            for (i, pair) in v.windows(2).enumerate() {
                let length = pair[0].distance_to(pair[1]);
                // A piece within the geometric tolerance has no "along", as for
                // nearest_anchor: the half landing in one goes to the next piece.
                if !polyline.segment(i).is_degenerate()
                    && (before + length >= half || i + 2 == v.len())
                {
                    reference.index = i as u32;
                    reference.parameter = cpp::clamp((half - before) / length, 0.0, 1.0);
                    return Some(reference);
                }
                before += length;
            }
            None
        }
        Geometry::CurvePolyline(curve) => {
            // As a polyline: inside when closed, else halfway along - found by
            // nearest_anchor from the point halfway, so an arc segment's
            // fraction is of its sweep.
            if curve.closed && curve.vertices.len() >= 3 {
                reference.point = AnchorPoint::Inside;
                return Some(reference);
            }
            let half = 0.5 * curve.length();
            if !(half > 0.0) {
                return None;
            }
            nearest_anchor(entity, curve.point_at_station(half))
        }
        Geometry::Ellipse(ellipse) => {
            if ellipse.is_full() {
                reference.point = AnchorPoint::Inside;
            } else {
                reference.parameter = 0.5;
            }
            Some(reference)
        }
        Geometry::Spline(_) => {
            // No place along a spline (nearest_anchor): its start.
            reference.point = AnchorPoint::Start;
            Some(reference)
        }
        Geometry::Dimension(_) | Geometry::Label(_) | Geometry::Leader(_) => None,
    }
}

// The context a refusal about leader `id` names.
fn refusal_context(id: EntityId, context: &str) -> String {
    if context.is_empty() {
        format!("leader={id}")
    } else {
        format!("leader={id} {context}")
    }
}

/// What of `change` can be refused without a leader: a template that does
/// not check (entity's check_leader_template), a text or label style not in
/// the model, a size below 0 mm (or not a number).
pub fn check_leader_change(model: &Model, change: &LeaderChange) -> Status {
    if let Some(note) = &change.note
        && note.kind == LeaderNoteKind::Template
    {
        check_leader_template(&note.text)?;
    }
    if let Some(note) = &change.note
        && note.kind == LeaderNoteKind::LabelStyle
        && !model.label_styles.contains(&note.text)
    {
        return Err(make_error(
            ErrorCode::NotFound,
            "label style does not exist",
            note.text.as_str(),
        ));
    }
    if let Some(style) = &change.text_style
        && !style.is_empty()
        && !model.text_styles.contains(style)
    {
        return Err(make_error(
            ErrorCode::NotFound,
            "text style does not exist",
            style.as_str(),
        ));
    }
    if (change.vertex.is_some() && change.hang.is_some())
        || (change.vertex.is_some_and(|vertex| vertex.index == 0) && change.tip.is_some())
    {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "a leader point cannot be assigned twice in one edit",
        ));
    }
    if let Some(Some(angle)) = change.landing_angle
        && !angle.is_finite()
    {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "landingangle= must be finite or auto",
        ));
    }
    // Millimetres on paper, where 0 already means something (the text
    // style's height, no landing): below it there is nothing to draw, and a
    // negative landing would hang the note back over its own line.
    for (size, key) in [
        (change.paper_height, "paper"),
        (change.arrow_size, "arrowsize"),
        (change.landing, "landing"),
    ] {
        if let Some(size) = size
            && !(size.is_finite() && size >= 0.0)
        {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                format!("{key}= must be a size of 0 mm or more"),
                format_exact_real(size),
            ));
        }
    }
    Ok(())
}

/// The point editor and grips share this operation. An attached tip must be
/// reattached or detached explicitly, never silently freed by a drag.
pub fn move_leader_vertices(
    leader: &LeaderGeometry,
    moves: &[LeaderVertexMove],
) -> Result<LeaderGeometry> {
    let mut moved = leader.clone();
    let mut changed = BTreeSet::new();
    for movement in moves {
        if movement.index >= moved.vertices.len() {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "leader vertex index is out of range",
                format!("{}", movement.index),
            ));
        }
        if !movement.position.is_finite() {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "leader point coordinates must be finite",
            ));
        }
        if !changed.insert(movement.index) {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "a leader vertex is moved more than once",
                format!("{}", movement.index),
            ));
        }
        if movement.index == 0
            && leader.tip_ref.associated()
            && movement.position != leader.vertices[0]
        {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "the leader tip is attached; use LEADER ATTACH or DETACH before moving it",
            ));
        }
        moved.vertices[movement.index] = movement.position;
    }
    if !Polyline2::new(moved.vertices.clone(), false)
        .length()
        .is_finite()
    {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the edited leader is too large",
        ));
    }
    validate(&Geometry::Leader(moved.clone()))?;
    Ok(moved)
}

/// "auto" (None, the automatic direction) or a finite angle in degrees, as
/// radians in (-pi, pi].
pub fn parse_leader_landing_angle(degrees: &str) -> Result<Option<f64>> {
    if equals_ignoring_case(degrees.as_bytes(), b"auto") {
        return Ok(None);
    }
    let Some(value) = parse_finite_double(degrees) else {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "landingangle= takes auto or a finite angle in degrees",
            degrees,
        ));
    };
    Ok(Some(cpp::remainder(value, 360.0) * DEG_TO_RAD))
}

/// `change` applied to `leader` (the leader with id `id`, or 0 for one not
/// made yet; 0 and `&LeaderContext::default()` are the C++ defaults). A
/// label-style note with no text style or paper height in the change lends
/// the leader the style's, so the leader looks as the style's labels do.
/// Refused: what check_leader_change refuses, a tip put on the leader itself,
/// and a smart leader that would say nothing (entity's
/// check_leader_says_something). The geometry's own validity is the
/// command's to check.
pub fn apply_leader_change(
    model: &Model,
    change: &LeaderChange,
    leader: &mut LeaderGeometry,
    id: EntityId,
    context: &LeaderContext,
) -> Status {
    check_leader_change(model, change)?;
    if let Some(note) = &change.note {
        match note.kind {
            LeaderNoteKind::Text => {
                leader.text = note.text.clone();
                leader.fields = false;
                leader.label_style.clear();
            }
            LeaderNoteKind::Template => {
                leader.text = note.text.clone();
                leader.fields = true;
                leader.label_style.clear();
            }
            LeaderNoteKind::LabelStyle => {
                leader.label_style = note.text.clone();
                leader.text.clear();
                leader.fields = false;
                // Lent so the leader looks as the style's labels do; after
                // that the look is the leader's own. The style is there:
                // check_leader_change has said so.
                if let Some(style) = model.label_styles.find(&note.text) {
                    if change.text_style.is_none() && !style.text_style.is_empty() {
                        leader.style = style.text_style.clone();
                    }
                    if change.paper_height.is_none() && style.paper_height > 0.0 {
                        leader.paper_height = style.paper_height;
                    }
                }
            }
        }
    }
    if let Some(arrow) = change.arrow {
        leader.arrow = arrow;
    }
    if let Some(callout) = change.callout {
        leader.callout = callout;
    }
    if let Some(style) = &change.text_style {
        leader.style = style.clone();
    }
    if let Some(height) = change.paper_height {
        leader.paper_height = height;
    }
    if let Some(size) = change.arrow_size {
        leader.arrow_size = size;
    }
    if let Some(landing) = change.landing {
        leader.landing = landing;
    }
    if let Some(angle) = change.landing_angle {
        leader.landing_angle = angle;
    }
    if let Some(tip) = &change.tip {
        if id != 0 && tip.r#ref.entity == id {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "a leader's tip cannot be on the leader itself",
                format!("id={id}"),
            ));
        }
        if leader.vertices.is_empty() {
            leader.vertices.push(tip.point);
        } else {
            leader.vertices[0] = tip.point;
        }
        leader.tip_ref = tip.r#ref;
    }
    if change.hang.is_some() || change.vertex.is_some() {
        if leader.vertices.is_empty() {
            return Err(Error::new(
                ErrorCode::InvalidGeometry,
                "the leader has no point to move",
            ));
        }
        let last = leader.vertices.len() - 1;
        let movement = change.vertex.or_else(|| {
            change.hang.map(|hang| LeaderVertexMove {
                index: last,
                position: hang,
            })
        });
        if let Some(movement) = movement {
            *leader = move_leader_vertices(leader, &[movement])?;
        }
    }
    check_leader_says_something(model, leader, code_properties(), context)
}

/// Whether every one of `ids` is a leader in the model; the first that is not
/// is named.
pub fn require_leaders(model: &Model, ids: &[EntityId]) -> Status {
    for &id in ids {
        let is_leader = model
            .entities
            .find(id)
            .is_some_and(|entity| matches!(entity.geometry, Geometry::Leader(_)));
        if !is_leader {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "that entity is not a leader",
                format!("id={id}"),
            ));
        }
    }
    Ok(())
}

/// A balloon (`balloon`) numbered for want of a note in `change` is refused
/// with a callout other than a circle: only a circle's number is counted, by
/// next_balloon_number and BALLOON RENUMBER, so another would be given again.
pub fn check_numbered_balloon(change: &LeaderChange, balloon: bool) -> Status {
    if balloon
        && change.note.is_none()
        && let Some(callout) = change.callout
        && callout != CalloutShape::Circle
    {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "a numbered balloon is a circle: leave callout= a circle, or give the balloon a note",
            format!("callout={}", to_string(callout)),
        ));
    }
    Ok(())
}

/// A new leader through `points` (the tip first): `change` applied to a plain
/// one, a tip Inside an outline ending in a dot unless the change gives an
/// arrow (ISO 128-22, leader lines), and a balloon a circle callout numbered
/// on from the highest when the change gives it no note - refused with any
/// other callout, since only a circle's number is counted.
pub fn new_leader(
    model: &Model,
    points: &[AnchoredPoint],
    change: &LeaderChange,
    balloon: bool,
    context: &LeaderContext,
) -> Result<LeaderGeometry> {
    check_numbered_balloon(change, balloon)?;
    let mut leader = LeaderGeometry::default();
    for point in points {
        leader.vertices.push(point.point);
    }
    if let Some(first) = points.first() {
        leader.tip_ref = first.r#ref; // the tip follows what it points at
    }
    if leader.tip_ref.point == AnchorPoint::Inside {
        // A leader ending inside an outline ends in a dot, one ending on it
        // in an arrowhead (ISO 128-22, leader lines).
        leader.arrow = ArrowHead::Dot;
    }
    if balloon {
        leader.callout = CalloutShape::Circle;
        if change.note.is_none() {
            let number = next_balloon_number(model)?;
            leader.text = format!("{number}");
        }
    }
    let mut rest = change.clone();
    rest.tip = None; // the points are the leader's already
    apply_leader_change(model, &rest, &mut leader, 0, context)?;
    Ok(leader)
}

/// ONE command changing each of `ids` by `change` ("SET_LEADER"); None when
/// nothing would change. `hang` and `vertex` are refused for more than one
/// leader, since it would put every note on one place.
pub fn change_leaders(
    model: &Model,
    ids: Vec<EntityId>,
    change: &LeaderChange,
    context: &LeaderContext,
) -> Result<Option<CommandPtr>> {
    let ids = unique(ids);
    if ids.is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "no leader to change",
        ));
    }
    require_leaders(model, &ids)?;
    if change.hang.is_some() && ids.len() != 1 {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "at= moves one leader's note; name one leader",
        ));
    }
    if change.vertex.is_some() && ids.len() != 1 {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "vertex= moves one leader's point; name one leader",
        ));
    }
    let mut changes = ChangeSet::default();
    for &id in &ids {
        let Some(original) = model.entities.find(id) else {
            continue;
        };
        let mut changed = original.clone();
        let leader = leader_of_mut(&mut changed);
        if let Err(error) = apply_leader_change(model, change, leader, id, context) {
            return Err(make_error(
                error.code,
                error.message,
                refusal_context(id, &error.context),
            ));
        }
        if changed == *original {
            continue;
        }
        changes.modify.push(changed);
    }
    if changes.empty() {
        return Ok(None);
    }
    Ok(Some(command_of("SET_LEADER", changes)))
}

/// The tip of leader `id` put on `place`, which must name an entity
/// ("ATTACH_LEADER"); None when the tip is on that place already.
pub fn attach_leader(
    model: &Model,
    id: EntityId,
    place: &AnchoredPoint,
    context: &LeaderContext,
) -> Result<Option<CommandPtr>> {
    if !place.r#ref.associated() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "ATTACH puts the tip on an entity: give #id, #id.end, #id.inside or #id@x,y",
        ));
    }
    require_leaders(model, &[id])?;
    let change = LeaderChange {
        tip: Some(*place),
        ..LeaderChange::default()
    };
    let Some(original) = model.entities.find(id) else {
        return Ok(None);
    };
    let mut changed = original.clone();
    apply_leader_change(model, &change, leader_of_mut(&mut changed), id, context)?;
    if changed == *original {
        return Ok(None); // on that place already: no step, as change_leaders
    }
    let changes = ChangeSet {
        modify: vec![changed],
        ..ChangeSet::default()
    };
    Ok(Some(command_of("ATTACH_LEADER", changes)))
}

/// FREEZE (detach false): a smart leader's note becomes the words it says
/// now, its tip still following. DETACH (detach true): the tip lets go, a
/// smart note frozen first, since a note read off nothing would say nothing.
/// The command is None when nothing changes.
#[derive(Default)]
pub struct LeaderRelease {
    pub command: Option<CommandPtr>,
    pub changed: usize,
    pub frozen: usize,
}

impl fmt::Debug for LeaderRelease {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LeaderRelease")
            .field(
                "command",
                &self.command.as_ref().map(|command| command.name()),
            )
            .field("changed", &self.changed)
            .field("frozen", &self.frozen)
            .finish()
    }
}

pub fn release_leaders(
    model: &Model,
    ids: Vec<EntityId>,
    detach: bool,
    context: &LeaderContext,
) -> Result<LeaderRelease> {
    let ids = unique(ids);
    require_leaders(model, &ids)?;
    let mut release = LeaderRelease::default();
    let mut changes = ChangeSet::default();
    for &id in &ids {
        let Some(original) = model.entities.find(id) else {
            continue;
        };
        let mut changed = original.clone();
        let leader = leader_of_mut(&mut changed);
        if detach && !leader.tip_ref.associated() {
            continue;
        }
        if is_smart(leader) {
            // Frozen as it reads now, a surface's level and an alignment's
            // station included.
            leader.text = leader_says(model, leader, context);
            leader.fields = false;
            leader.label_style.clear();
            release.frozen += 1;
        } else if !detach {
            continue;
        }
        if detach {
            leader.tip_ref = AnchorRef::default();
        }
        changes.modify.push(changed);
    }
    release.changed = changes.modify.len();
    if !changes.empty() {
        release.command = Some(command_of(
            if detach {
                "DETACH_LEADER"
            } else {
                "FREEZE_LEADER"
            },
            changes,
        ));
    }
    Ok(release)
}

/// The attribute `key` of the entity leader `id`'s tip is on: set to
/// `value`, or removed when there is none. The same commands PROP SET and
/// PROP DELETE run.
pub fn set_leader_target_property(
    model: &Model,
    id: EntityId,
    key: &str,
    value: Option<&PropertyValue>,
) -> Result<CommandPtr> {
    Ok(set_leaders_target_property(model, vec![id], key, value)?.command)
}

/// The same for many leaders (LEADER PROP over a scope): the attribute set,
/// or removed, on the entity each of `ids`' tips is on, as ONE command, each
/// entity once - two leaders on one pit set it once. A leader whose tip is on
/// no entity, or on one that is gone, is skipped and counted; when NO leader
/// has an entity to set it on the edit is refused with the reason the first
/// gave, which for one leader is set_leader_target_property's.
pub struct LeadersProperty {
    pub command: CommandPtr,
    /// The leaders whose entity was set, and the distinct entities they name.
    pub leaders: usize,
    pub entities: usize,
    /// The leaders left out for want of an entity.
    pub skipped: usize,
}

impl fmt::Debug for LeadersProperty {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LeadersProperty")
            .field("command", &self.command.name())
            .field("leaders", &self.leaders)
            .field("entities", &self.entities)
            .field("skipped", &self.skipped)
            .finish()
    }
}

pub fn set_leaders_target_property(
    model: &Model,
    ids: Vec<EntityId>,
    key: &str,
    value: Option<&PropertyValue>,
) -> Result<LeadersProperty> {
    let ids = unique(ids);
    if ids.is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "no leader to set an attribute through",
        ));
    }
    require_leaders(model, &ids)?;
    let mut leaders = 0;
    let mut skipped = 0;
    let mut entities = Vec::new();
    // The first leader left out says why: that reason is what a single
    // leader's refusal is.
    let mut first_reason: Status = Ok(());
    for &id in &ids {
        let tip_ref = leader_in(model, id).tip_ref;
        if !tip_ref.associated() {
            skipped += 1;
            if first_reason.is_ok() {
                first_reason = Err(make_error(
                    ErrorCode::InvalidArgument,
                    "that leader's tip is on no entity, so it has no attributes to set",
                    format!("id={id}"),
                ));
            }
            continue;
        }
        if !model.entities.contains(tip_ref.entity) {
            skipped += 1;
            if first_reason.is_ok() {
                first_reason = Err(make_error(
                    ErrorCode::NotFound,
                    "the entity the leader's tip is on is gone",
                    format!("id={}", tip_ref.entity),
                ));
            }
            continue;
        }
        entities.push(tip_ref.entity);
        leaders += 1;
    }
    if leaders == 0 {
        // Every leader was left out, so the first reason is a refusal.
        first_reason?;
    }
    if key.is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "an attribute needs a name",
        ));
    }
    let entities = unique(entities);
    let count = entities.len();
    let command = match value {
        Some(value) => set_entity_property(entities, key, value.clone()),
        None => remove_entity_property(entities, key),
    };
    Ok(LeadersProperty {
        command,
        leaders,
        entities: count,
        skipped,
    })
}

/// A leader to each of `targets` as ONE command ("CREATE_LEADERS" or
/// "CREATE_BALLOONS"): the tip at a point's or a text's position, the middle
/// of a line or an arc, halfway along an open polyline, inside a closed one,
/// on a circle on the side the note goes; the note `length` paper
/// millimetres from the tip at `angle` (radians). An entity that offers no
/// place, or that the note would say nothing about, is skipped and counted;
/// refused when none is made, with the first reason, and - as new_leader - a
/// balloon numbered for want of a note with a callout that is not a circle.
#[derive(Clone, Debug, PartialEq)]
pub struct LeadersForOptions {
    pub change: LeaderChange,
    /// Chosen, not a standard's. Up and to the right at 45 degrees: off the
    /// horizontal and vertical most drawn lines follow, so a leader crosses
    /// neither along its length.
    pub angle: f64,
    /// Paper millimetres: four of a leader's default 2.5 mm text heights
    /// (LeaderGeometry::paper_height's fallback), so the note sits clear of
    /// the tip and of a point's symbol there.
    pub length: f64,
    pub balloon: bool,
}

impl Default for LeadersForOptions {
    fn default() -> Self {
        Self {
            change: LeaderChange::default(),
            angle: 0.25 * PI,
            length: 10.0,
            balloon: false,
        }
    }
}

pub struct LeadersFor {
    pub command: CommandPtr,
    pub made: usize,
    pub skipped: usize,
    pub first_skip: String,
}

impl fmt::Debug for LeadersFor {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LeadersFor")
            .field("command", &self.command.name())
            .field("made", &self.made)
            .field("skipped", &self.skipped)
            .field("first_skip", &self.first_skip)
            .finish()
    }
}

pub fn leaders_for(
    model: &Model,
    targets: Vec<EntityId>,
    options: &LeadersForOptions,
    scale: f64,
    attributes: &EntityAttributes,
    context: &LeaderContext,
) -> Result<LeadersFor> {
    let targets = unique(targets);
    if targets.is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "no entity to make leaders for",
        ));
    }
    if !(options.length > 0.0) || !options.length.is_finite() {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "length= must be more than zero",
            format_exact_real(options.length),
        ));
    }
    if !options.angle.is_finite() || !(scale > 0.0) {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the angle and the scale must be numbers",
        ));
    }
    let to_note = Vec2::new(options.angle.cos(), options.angle.sin())
        * katana_entity::annotation::annotation_model_size(options.length, scale);
    // What does not depend on the entity is refused once, for all of them.
    check_numbered_balloon(&options.change, options.balloon)?;
    check_leader_change(model, &options.change)?;
    let numbered = options.balloon && options.change.note.is_none();
    let mut first: i64 = 0;
    if numbered {
        first = next_balloon_number(model)?;
        // Each target may take a number: the last must still be one.
        if (targets.len() - 1) as u64 > (i64::MAX - first) as u64 {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "the balloons would be numbered past the largest number there is: \
                 BALLOON RENUMBER first",
                format!("from={first}"),
            ));
        }
    }
    let mut skipped = 0;
    let mut first_skip = String::new();
    let mut changes = ChangeSet::default();
    for &id in &targets {
        let Some(target) = model.entities.find(id) else {
            return Err(make_error(
                ErrorCode::NotFound,
                "entity does not exist",
                format!("id={id}"),
            ));
        };
        let Some(place) = leader_place_on(target, options.angle) else {
            skipped += 1;
            if first_skip.is_empty() {
                first_skip = format!(
                    "id={id} (a {}) has no place to attach a leader to",
                    to_string(target.r#type())
                );
            }
            continue;
        };
        let mut leader = LeaderGeometry {
            vertices: vec![place.point, place.point + to_note],
            tip_ref: place.r#ref,
            ..LeaderGeometry::default()
        };
        if place.r#ref.point == AnchorPoint::Inside {
            leader.arrow = ArrowHead::Dot; // ISO 128-22, leader lines
        }
        if options.balloon {
            leader.callout = CalloutShape::Circle;
        }
        let mut change = options.change.clone();
        change.tip = None;
        change.hang = None;
        if numbered {
            // On from the first, by the balloons made so far.
            change.note = Some(LeaderNote {
                kind: LeaderNoteKind::Text,
                text: format!("{}", first + changes.add.len() as i64),
            });
        }
        if let Err(error) = apply_leader_change(model, &change, &mut leader, 0, context) {
            // Checked above for what does not depend on the entity, so this
            // is a note that would say nothing about this one: skipped.
            skipped += 1;
            if first_skip.is_empty() {
                first_skip = format!("{} [{}]", error.message, error.context);
            }
            continue;
        }
        changes.add.push(Entity {
            geometry: Geometry::Leader(leader),
            layer: attributes.layer.clone(),
            style: attributes.style.clone(),
            color: attributes.color,
            ..Entity::default()
        });
    }
    if changes.add.is_empty() {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "no leader made",
            first_skip,
        ));
    }
    let made = changes.add.len();
    Ok(LeadersFor {
        command: command_of(
            if options.balloon {
                "CREATE_BALLOONS"
            } else {
                "CREATE_LEADERS"
            },
            changes,
        ),
        made,
        skipped,
        first_skip,
    })
}

/// Where leaders_for puts a leader's tip on `entity` for a note in direction
/// `angle` (radians): the place, named so the tip follows it, and the point.
/// None for an entity that offers no place (a dimension, a label, a leader).
/// What the window previews a leader for the selection from.
pub fn leader_place_on(entity: &Entity, angle: f64) -> Option<AnchoredPoint> {
    let reference = natural_anchor(entity, angle)?;
    let point = resolve_anchor(entity, &reference)?;
    Some(AnchoredPoint {
        point,
        r#ref: reference,
    })
}

/// Where a tip at `tip` goes when put on `entity` (the window's Attach to
/// Selected): Inside a closed outline or a circle the tip is inside -
/// pointing into a lot, as LEADER FOR does for one - else the place of
/// `entity` nearest the tip (entity's nearest_anchor). None for an entity
/// that offers no place.
pub fn tip_place_on(entity: &Entity, tip: Point2) -> Option<AnchoredPoint> {
    let inside = match &entity.geometry {
        Geometry::Polyline(outline) => {
            outline.closed
                && outline.vertices.len() >= 3
                && outline.classify(tip) == Containment::Inside
        }
        Geometry::Circle(circle) => circle.classify(tip) == Containment::Inside,
        Geometry::CurvePolyline(curve) => {
            curve.closed
                && curve.vertices.len() >= 3
                && curve.to_polyline(CURVE_CHORD_TOLERANCE).classify(tip) == Containment::Inside
        }
        Geometry::Ellipse(ellipse) => {
            // Inside a whole ellipse: (u/a)^2 + (v/b)^2 < 1 in its own axes.
            let a = ellipse.major_radius();
            let b = ellipse.minor_radius();
            if ellipse.is_full() && a > 0.0 && b > 0.0 {
                let d = tip - ellipse.center;
                let u = d.dot(ellipse.major_axis) / a;
                let v = d.dot(ellipse.major_axis.perpendicular()) / a;
                (u / a) * (u / a) + (v / b) * (v / b) < 1.0
            } else {
                false
            }
        }
        _ => false,
    };
    let reference = if inside {
        Some(AnchorRef {
            entity: entity.id,
            point: AnchorPoint::Inside,
            ..AnchorRef::default()
        })
    } else if matches!(entity.geometry, Geometry::Spline(_)) {
        // No place along a spline (entity's nearest_anchor): its start, as
        // LEADER FOR puts one.
        Some(AnchorRef {
            entity: entity.id,
            point: AnchorPoint::Start,
            ..AnchorRef::default()
        })
    } else {
        nearest_anchor(entity, tip)
    }?;
    let point = resolve_anchor(entity, &reference)?;
    Some(AnchoredPoint {
        point,
        r#ref: reference,
    })
}

/// The notes of `ids` in a column ("ALIGN_LEADERS"): top down by where each
/// hangs (its last vertex), each moved to `x` (the topmost's when unset) and,
/// with `spacing` (model units), stacked that far apart from the top one's
/// height. The command is None when nothing moves.
pub struct LeaderAlignment {
    pub command: Option<CommandPtr>,
    pub x: f64,
    pub count: usize,
}

impl fmt::Debug for LeaderAlignment {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LeaderAlignment")
            .field(
                "command",
                &self.command.as_ref().map(|command| command.name()),
            )
            .field("x", &self.x)
            .field("count", &self.count)
            .finish()
    }
}

// A "less than" made a sort's Ordering: what std::stable_sort is given.
fn ordering(a_first: bool, b_first: bool) -> Ordering {
    if a_first {
        Ordering::Less
    } else if b_first {
        Ordering::Greater
    } else {
        Ordering::Equal
    }
}

pub fn align_leaders(
    model: &Model,
    ids: Vec<EntityId>,
    x: Option<f64>,
    spacing: Option<f64>,
) -> Result<LeaderAlignment> {
    let mut ids = unique(ids);
    if ids.is_empty() {
        return Err(Error::new(ErrorCode::InvalidArgument, "no leader to align"));
    }
    require_leaders(model, &ids)?;
    if let Some(spacing) = spacing
        && !(spacing.is_finite() && spacing > 0.0)
    {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "spacing= must be a size more than zero",
        ));
    }
    if let Some(x) = x
        && !x.is_finite()
    {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "x= must be a number",
        ));
    }
    // Top to bottom by where each note hangs, then by id: a stable sort.
    let hang_y = |id: EntityId| {
        leader_in(model, id)
            .vertices
            .last()
            .map_or(0.0, |point| point.y)
    };
    ids.sort_by(|&a, &b| {
        let (ya, yb) = (hang_y(a), hang_y(b));
        ordering(ya > yb, yb > ya)
    });
    let top = leader_in(model, ids[0])
        .vertices
        .last()
        .copied()
        .unwrap_or_default();
    // A spacing each finite can still stack the lowest note past any number.
    if let Some(spacing) = spacing
        && !(top.y - (ids.len() - 1) as f64 * spacing).is_finite()
    {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "spacing= stacks the notes further than a drawing reaches",
            format!("spacing={}", format_exact_real(spacing)),
        ));
    }
    let aligned_x = x.unwrap_or(top.x);
    let mut changes = ChangeSet::default();
    for (k, &id) in ids.iter().enumerate() {
        let Some(original) = model.entities.find(id) else {
            continue;
        };
        let mut changed = original.clone();
        let leader = leader_of_mut(&mut changed);
        let Some(hang) = leader.vertices.last_mut() else {
            continue;
        };
        let moved = Point2::new(
            aligned_x,
            match spacing {
                Some(spacing) => top.y - k as f64 * spacing,
                None => hang.y,
            },
        );
        if moved == *hang {
            continue;
        }
        *hang = moved;
        changes.modify.push(changed);
    }
    Ok(LeaderAlignment {
        command: if changes.empty() {
            None
        } else {
            Some(command_of("ALIGN_LEADERS", changes))
        },
        x: aligned_x,
        count: ids.len(),
    })
}

/// The numbered balloons - a circled plain leader saying a whole number -
/// numbered again from `start` ("RENUMBER_BALLOONS"): in the order they were
/// made, across the sheet by their tips (X, left to right) or down it (Y, top
/// down), ties in id order. The command is None when every one already says
/// its number.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum BalloonOrder {
    #[default]
    Id,
    X,
    Y,
}

#[derive(Default)]
pub struct BalloonRenumbering {
    pub command: Option<CommandPtr>,
    pub balloons: usize,
    pub renumbered: usize,
}

impl fmt::Debug for BalloonRenumbering {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BalloonRenumbering")
            .field(
                "command",
                &self.command.as_ref().map(|command| command.name()),
            )
            .field("balloons", &self.balloons)
            .field("renumbered", &self.renumbered)
            .finish()
    }
}

/// `start` is an i32, as start= and the window's box are, so the numbers
/// counted on from it cannot pass what an i64 holds. `within` limits it to
/// the numbered balloons among those ids (a scope's matches); None, every
/// numbered balloon in the drawing, and empty, none - the count then 0. The
/// balloons outside it keep their numbers, so a renumbering of some can give
/// a number another balloon has: the caller says so.
pub fn renumber_balloons(
    model: &Model,
    start: i32,
    order: BalloonOrder,
    within: Option<&[EntityId]>,
) -> BalloonRenumbering {
    // The ids are searched as they are given: a scope's matches arrive in id
    // order, and a caller's own list need not.
    let allowed = within.map(|ids| unique(ids.to_vec()));
    let mut balloons = Vec::new();
    model.entities.for_each(|entity| {
        if let Geometry::Leader(leader) = &entity.geometry
            && is_numbered_balloon(leader)
            && allowed
                .as_ref()
                .is_none_or(|allowed| allowed.binary_search(&entity.id).is_ok())
        {
            balloons.push(entity.id);
        }
    });
    if order != BalloonOrder::Id {
        let tip = |id: EntityId| {
            leader_in(model, id)
                .vertices
                .first()
                .copied()
                .unwrap_or_default()
        };
        balloons.sort_by(|&a, &b| {
            let (p, q) = (tip(a), tip(b));
            if order == BalloonOrder::X {
                ordering(p.x < q.x, q.x < p.x)
            } else {
                ordering(p.y > q.y, q.y > p.y)
            }
        });
    }
    let mut result = BalloonRenumbering {
        balloons: balloons.len(),
        ..BalloonRenumbering::default()
    };
    let mut changes = ChangeSet::default();
    for (k, &id) in balloons.iter().enumerate() {
        let Some(original) = model.entities.find(id) else {
            continue;
        };
        let mut changed = original.clone();
        let leader = leader_of_mut(&mut changed);
        let number = format!("{}", i64::from(start) + k as i64);
        if leader.text == number {
            continue;
        }
        leader.text = number;
        changes.modify.push(changed);
    }
    result.renumbered = changes.modify.len();
    if !changes.empty() {
        result.command = Some(command_of("RENUMBER_BALLOONS", changes));
    }
    result
}

/// One more than the highest numbered balloon; refused when the highest is
/// the largest number there is (n= can give one), which has no next - the
/// balloons want numbering again first.
pub fn next_balloon_number(model: &Model) -> Result<i64> {
    let mut highest: i64 = 0;
    model.entities.for_each(|entity| {
        if let Geometry::Leader(leader) = &entity.geometry
            && is_numbered_balloon(leader)
        {
            let value = whole_number(&leader.text).unwrap_or(0);
            highest = std::cmp::max(highest, value);
        }
    });
    if highest == i64::MAX {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "the highest balloon is numbered as high as a number goes, so has no next: \
             BALLOON RENUMBER first",
            format!("n={highest}"),
        ));
    }
    Ok(highest + 1)
}

/// A leader's or a place's value as a row of name and text, in its own
/// format.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LeaderValueRow {
    pub name: String,
    pub text: String,
}

/// The values as rows: leader_value_names() order, then the context's
/// (surface.NAME, ..., by kind in leader_context_kinds() order, then by
/// name), then the properties in name order. What LEADER VALUES replies and
/// the Leaders manager lists.
pub fn leader_value_rows(values: &LabelValues) -> Vec<LeaderValueRow> {
    let mut rows = Vec::new();
    for name in leader_value_names() {
        if let Some(found) = values.get(name) {
            rows.push(LeaderValueRow {
                name: String::from(name),
                text: format_value(found),
            });
        }
    }
    // The surfaces' and the alignments', kind by kind - every surface's
    // level, then every difference - each kind's in name order.
    for kind in leader_context_kinds() {
        for (name, value) in values {
            if context_value_name(name).is_some_and(|parsed| parsed.kind == kind) {
                rows.push(LeaderValueRow {
                    name: name.clone(),
                    text: format_value(value),
                });
            }
        }
    }
    for (name, value) in values {
        if name.starts_with("prop.") {
            rows.push(LeaderValueRow {
                name: name.clone(),
                text: format_value(value),
            });
        }
    }
    rows
}

/// The values of the entity a leader's tip is on, at the tip, with the
/// survey code looked for where survey coding looks and every value of the
/// context; None for a leader on nothing, or on an entity that is gone.
pub fn leader_target_values(
    model: &Model,
    leader: &LeaderGeometry,
    context: &LeaderContext,
) -> Option<LabelValues> {
    leader_values(model, leader, code_properties(), context)
}

/// The note a leader says now, the survey code looked for where survey
/// coding looks: entity's leader_note with cad's code properties.
pub fn leader_says(model: &Model, leader: &LeaderGeometry, context: &LeaderContext) -> String {
    leader_note(model, leader, code_properties(), context)
}

// ---- the surfaces and alignments a note reads, by name -------------------------------------
//
// A note names a surface or an alignment as the drawing spells it
// ({dsurface.Existing Ground:+.3f}), so the name is what ties them. These keep
// the tie when the thing is renamed, and say how many leaders lose it when the
// thing goes (docs/annotation.md, "A surface or an alignment is read by
// name").

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ReadKind {
    Surface,
    Alignment,
}

// A surface is found ignoring case (SurfaceStore::find, which is what a note's
// {surface.NAME} reads through), an alignment exactly (ALIGN).
fn same_name(kind: ReadKind, a: &str, b: &str) -> bool {
    if kind == ReadKind::Surface {
        equals_ignoring_case(a.as_bytes(), b.as_bytes())
    } else {
        a == b
    }
}

// Whether the template field `field` ("dsurface.Existing Ground") reads
// `name` of `kind`.
fn field_reads(kind: ReadKind, name: &str, field: &str) -> bool {
    context_value_name(field).is_some_and(|parsed| {
        is_surface_context_kind(parsed.kind) == (kind == ReadKind::Surface)
            && same_name(kind, parsed.name, name)
    })
}

fn template_reads(kind: ReadKind, name: &str, template_text: &str) -> bool {
    template_fields(template_text)
        .iter()
        .any(|field| field_reads(kind, name, field))
}

/// How many leaders have a template of their own naming `name`: a surface
/// ignoring case, as the store finds one, an alignment exactly, as ALIGN
/// does.
pub fn leaders_reading(model: &Model, kind: ReadKind, name: &str) -> usize {
    let mut count = 0;
    model.entities.for_each(|entity| {
        if let Geometry::Leader(leader) = &entity.geometry
            && leader.fields
            && template_reads(kind, name, &leader.text)
        {
            count += 1;
        }
    });
    count
}

/// The leaders reading `from` rewritten to read `to`, as ONE command
/// ("RENAME_LEADER_READS") for the step that renames it to run with; None
/// when none reads it. `followed` is how many were rewritten. A name a
/// template cannot write - empty, or holding ':', '{' or '}' (preset_template
/// refuses the same) - cannot be followed to: those leaders are left reading
/// the old name and counted in `unfollowed`, for the caller to say.
#[derive(Default)]
pub struct LeaderReadsRenamed {
    pub command: Option<CommandPtr>,
    pub followed: usize,
    pub unfollowed: usize,
}

impl fmt::Debug for LeaderReadsRenamed {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LeaderReadsRenamed")
            .field(
                "command",
                &self.command.as_ref().map(|command| command.name()),
            )
            .field("followed", &self.followed)
            .field("unfollowed", &self.unfollowed)
            .finish()
    }
}

pub fn rename_leader_reads(
    model: &Model,
    kind: ReadKind,
    from: &str,
    to: &str,
) -> LeaderReadsRenamed {
    let mut result = LeaderReadsRenamed::default();
    let writable = !to.is_empty() && !to.contains([':', '{', '}']);
    let mut changes = ChangeSet::default();
    model.entities.for_each(|entity| {
        let Geometry::Leader(leader) = &entity.geometry else {
            return;
        };
        if !leader.fields || !template_reads(kind, from, &leader.text) {
            return;
        }
        // On a locked layer (its own or a parent's) a leader cannot be
        // changed, and one leader in the way would refuse the whole rename:
        // it is left reading the old name, counted.
        let layer = model.layers.resolve(&entity.layer);
        if !writable || layer.layer.is_none() || layer.locked {
            result.unfollowed += 1;
            return;
        }
        let rewritten = rename_template_fields(&leader.text, &|field: &str| {
            if !field_reads(kind, from, field) {
                return None;
            }
            let parsed = context_value_name(field)?;
            Some(format!("{}.{to}", parsed.kind))
        });
        if rewritten == leader.text {
            return; // renamed to its own spelling: nothing to say
        }
        let mut changed = entity.clone();
        leader_of_mut(&mut changed).text = rewritten;
        changes.modify.push(changed);
        result.followed += 1;
    });
    if !changes.empty() {
        result.command = Some(command_of("RENAME_LEADER_READS", changes));
    }
    result
}
