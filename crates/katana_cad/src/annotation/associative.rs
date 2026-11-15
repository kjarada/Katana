//! The associative update: annotation that follows the geometry it refers to
//! (docs/annotation.md, "Associativity"; include/katana/cad/annotation/associative.hpp).
//!
//! A dimension whose points name other entities (AnchorRef), a leader whose
//! tip does, and a label (which always names its target) must follow those
//! entities through every edit - a MOVE, a grip drag, an OFFSET's undo, a
//! SET_GEOMETRY from an agent. Rather than teach every command about them,
//! Document::execute wraps every command in with_associative_update: after the
//! command runs, associative_changes works out what the annotations that
//! follow it now are, and applies that as a second change set INSIDE the same
//! undo step. One undo puts back the geometry and everything that followed it,
//! exactly (before-images, as every change set undoes).
//!
//! What follows what:
//!
//! * a dimension's start, end and vertex follow the anchors they name; a
//!   radial dimension whose vertex names a circle or an arc keeps its
//!   direction from the centre and takes the curve's new radius;
//! * a leader's tip follows its anchor;
//! * a label's anchor follows its target (a dragged label's position moves by
//!   as much, so it keeps its place beside the target), and a label whose
//!   target is gone is REMOVED - a label goes with what it labels, and comes
//!   back with it on undo;
//! * a reference to an entity that is gone is dropped, leaving the point where
//!   it last was: the dimension stays and is no longer associative, as a CAD
//!   user expects when a measured line is erased.
//!
//! An annotation on a locked layer is left as it is: the lock is the user
//! saying not to change it.

use std::collections::BTreeSet;

use katana_commands::{ChangeSet, ChangeSetCommand, Command, CommandContext, CommandPtr};
use katana_core::Status;
use katana_entity::anchor::resolve_anchor;
use katana_entity::label_values::label_anchor;
use katana_entity::leader_values::is_smart;
use katana_entity::{
    AnchorRef, DimensionGeometry, DimensionKind, EntityId, Geometry, Model, validate,
};
use katana_geometry::{Point2, Vec2};
use katana_math::numerics::tolerance;

/// Follows one reference: the point it names now, or the point unchanged with
/// the reference dropped when its entity is gone. `changed` is set when either
/// moved.
fn follow(model: &Model, reference: &mut AnchorRef, point: &mut Point2, changed: &mut bool) {
    if !reference.associated() {
        return;
    }
    let Some(target) = model.entities.find(reference.entity) else {
        *reference = AnchorRef::default();
        *changed = true;
        return;
    };
    if let Some(resolved) = resolve_anchor(target, reference)
        && !(resolved == *point)
    {
        *point = resolved;
        *changed = true;
    }
}

fn follow_dimension(model: &Model, dimension: &DimensionGeometry) -> Option<DimensionGeometry> {
    if !dimension.start_ref.associated()
        && !dimension.end_ref.associated()
        && !dimension.vertex_ref.associated()
    {
        return None;
    }
    let mut result = dimension.clone();
    let mut changed = false;
    let radial = matches!(
        dimension.kind,
        DimensionKind::Radius | DimensionKind::Diameter
    );
    // A radial dimension on a circle or an arc keeps its direction from the
    // centre and takes the curve's radius: the point it was put at is not an
    // anchor the curve offers, but "the curve, in this direction" is.
    if radial && dimension.vertex_ref.associated() && !dimension.start_ref.associated() {
        if let Some(curve) = model.entities.find(dimension.vertex_ref.entity) {
            let circle = match &curve.geometry {
                Geometry::Circle(c) => Some((c.center, c.radius)),
                Geometry::Arc(a) => Some((a.center, a.radius)),
                _ => None,
            };
            if let Some((center, radius)) = circle {
                let mut direction = dimension.start - dimension.vertex;
                direction = if direction.length() > 0.0 {
                    direction.normalized()
                } else {
                    Vec2::new(1.0, 0.0)
                };
                let start = center + direction * radius;
                if !(start == result.start) || !(center == result.vertex) {
                    result.start = start;
                    result.vertex = center;
                    changed = true;
                }
            } else {
                follow(
                    model,
                    &mut result.vertex_ref,
                    &mut result.vertex,
                    &mut changed,
                );
            }
        } else {
            result.vertex_ref = AnchorRef::default();
            changed = true;
        }
    } else {
        follow(
            model,
            &mut result.vertex_ref,
            &mut result.vertex,
            &mut changed,
        );
    }
    follow(
        model,
        &mut result.start_ref,
        &mut result.start,
        &mut changed,
    );
    follow(model, &mut result.end_ref, &mut result.end, &mut changed);
    // An angle between two lines (dimension_build, angular_between_lines) has
    // its vertex where the lines meet, which no anchor names: it is worked out
    // again from the lines whenever they move.
    if dimension.kind == DimensionKind::Angular
        && !result.vertex_ref.associated()
        && result.start_ref.associated()
        && result.end_ref.associated()
    {
        let line_of = |id: EntityId| match model.entities.find(id).map(|entity| &entity.geometry) {
            Some(Geometry::Line(segment)) => Some(*segment),
            _ => None,
        };
        if let (Some(a), Some(b)) = (
            line_of(result.start_ref.entity),
            line_of(result.end_ref.entity),
        ) {
            let da = a.end - a.start;
            let db = b.end - b.start;
            let denominator = da.cross(db);
            if denominator.abs() > tolerance::ANGULAR * da.length() * db.length() {
                let vertex = a.start + da * ((b.start - a.start).cross(db) / denominator);
                if !(vertex == result.vertex) {
                    result.vertex = vertex;
                    changed = true;
                }
            }
        }
    }
    if !changed {
        return None;
    }
    // Geometry the dimension cannot measure - two anchors brought together -
    // keeps the last good shape, only dropping references that are gone.
    if validate(&Geometry::Dimension(result.clone())).is_err() {
        let mut kept = dimension.clone();
        for (reference, now) in [
            (&mut kept.start_ref, &result.start_ref),
            (&mut kept.end_ref, &result.end_ref),
            (&mut kept.vertex_ref, &result.vertex_ref),
        ] {
            if !now.associated() {
                *reference = AnchorRef::default();
            }
        }
        return if kept == *dimension { None } else { Some(kept) };
    }
    Some(result)
}

/// A smart leader whose target this update removes - a label, or another
/// smart leader - goes too, however long the chain: removed ids are gathered
/// until a pass adds none, and a leader that was to follow a target now
/// removed is removed instead. Only smart leaders chain, since only they read
/// an annotation as their target; the extra passes run only when the first
/// removed something.
fn remove_smart_leaders_of_removed(model: &Model, changes: &mut ChangeSet) {
    if changes.remove.is_empty() {
        return;
    }
    let mut removed: BTreeSet<EntityId> = changes.remove.iter().copied().collect();
    let mut grew = true;
    while grew {
        grew = false;
        model.entities.for_each(|entity| {
            let Geometry::Leader(leader) = &entity.geometry else {
                return;
            };
            if !is_smart(leader)
                || !leader.tip_ref.associated()
                || removed.contains(&entity.id)
                || !removed.contains(&leader.tip_ref.entity)
                || model.layers.resolve(&entity.layer).locked
            {
                return;
            }
            removed.insert(entity.id);
            changes.remove.push(entity.id);
            grew = true;
        });
    }
    changes
        .modify
        .retain(|entity| !removed.contains(&entity.id));
}

/// What the annotations that follow other entities must become for the model
/// as it is now; empty when every one already agrees. Costs one pass over the
/// entities, and the resolution of each annotation that names another entity.
pub fn associative_changes(model: &Model) -> ChangeSet {
    let mut changes = ChangeSet::default();
    model.entities.for_each(|entity| {
        let follows = matches!(
            entity.geometry,
            Geometry::Label(_) | Geometry::Dimension(_) | Geometry::Leader(_)
        );
        if !follows || model.layers.resolve(&entity.layer).locked {
            return;
        }
        match &entity.geometry {
            Geometry::Label(label) => {
                let target_gone = if label.target != 0 {
                    !model.entities.contains(label.target)
                } else {
                    !model.alignments.contains(&label.alignment)
                };
                if target_gone {
                    changes.remove.push(entity.id);
                    return;
                }
                let Some(style) = model.label_styles.find(&label.style) else {
                    return;
                };
                let Some(anchor) = label_anchor(model, label, style) else {
                    return;
                };
                if anchor == label.anchor {
                    return;
                }
                let mut moved = entity.clone();
                if let Geometry::Label(updated) = &mut moved.geometry {
                    if let Some(position) = &mut updated.position {
                        *position += anchor - label.anchor;
                    }
                    updated.anchor = anchor;
                }
                changes.modify.push(moved);
            }
            Geometry::Dimension(dimension) => {
                if let Some(followed) = follow_dimension(model, dimension) {
                    let mut moved = entity.clone();
                    moved.geometry = Geometry::Dimension(followed);
                    changes.modify.push(moved);
                }
            }
            Geometry::Leader(leader) => {
                if !leader.tip_ref.associated() {
                    return;
                }
                // A smart leader's note is read off its target (docs/annotation.md,
                // "Smart leaders"): it goes with it, as a label does, and comes
                // back with it on undo. A plain one stays and lets the
                // reference go.
                if is_smart(leader) && !model.entities.contains(leader.tip_ref.entity) {
                    changes.remove.push(entity.id);
                    return;
                }
                let mut followed = leader.clone();
                let mut changed = false;
                if let Some(tip) = followed.vertices.first_mut() {
                    follow(model, &mut followed.tip_ref, tip, &mut changed);
                }
                if changed {
                    if validate(&Geometry::Leader(followed.clone())).is_err() {
                        // A tip brought onto the next vertex: keep the leader as
                        // it was.
                        followed = leader.clone();
                        followed.tip_ref = if model.entities.contains(leader.tip_ref.entity) {
                            leader.tip_ref
                        } else {
                            AnchorRef::default()
                        };
                        if followed == *leader {
                            return;
                        }
                    }
                    let mut moved = entity.clone();
                    moved.geometry = Geometry::Leader(followed);
                    changes.modify.push(moved);
                }
            }
            _ => {}
        }
    });
    remove_smart_leaders_of_removed(model, &mut changes);
    changes
}

struct AssociativeCommand {
    inner: CommandPtr,
    follow: Option<ChangeSetCommand>,
}

impl Command for AssociativeCommand {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn is_destructive(&self) -> bool {
        self.inner.is_destructive()
    }

    fn validate(&self, context: &CommandContext<'_>) -> Status {
        self.inner.validate(context)
    }

    fn execute(&mut self, context: &mut CommandContext<'_>) -> Status {
        self.inner.execute(context)?;
        let changes = associative_changes(context.model);
        if changes.empty() {
            return Ok(());
        }
        let mut follow = ChangeSetCommand::new(
            "ASSOCIATIVE_UPDATE",
            move |_: &CommandContext<'_>| Ok(changes.clone()),
            false,
        );
        if let Err(error) = follow.execute(context) {
            let _ = self.inner.undo(context);
            return Err(error);
        }
        self.follow = Some(follow);
        Ok(())
    }

    fn undo(&mut self, context: &mut CommandContext<'_>) -> Status {
        if let Some(follow) = &mut self.follow {
            follow.undo(context)?;
        }
        self.inner.undo(context)
    }

    fn redo(&mut self, context: &mut CommandContext<'_>) -> Status {
        self.inner.redo(context)?;
        match &mut self.follow {
            Some(follow) => follow.redo(context),
            None => Ok(()),
        }
    }

    fn created_entities(&self) -> Vec<EntityId> {
        self.inner.created_entities()
    }
}

/// `command`, followed within the same undo step by the associative update of
/// what it changed. Its name, destructiveness and created entities are the
/// command's own. If the update cannot be applied the command is undone and
/// the update's error returned, so the model is never left half followed.
pub fn with_associative_update(command: CommandPtr) -> CommandPtr {
    Box::new(AssociativeCommand {
        inner: command,
        follow: None,
    })
}
