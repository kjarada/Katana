//! The checks a project's contents pass before they are applied or saved, on entity ids in every
//! order (docs/performance.md, "Checking a project's contents"; CLAUDE.md section 5).
//!
//! The check keeps the ids it has met as a sorted vector while they ascend - the order a
//! project's entities come in - and falls back to a set for those that do not. What it refuses
//! must not depend on the order: here each generated list of ids is judged twice, by the check
//! and by an ordered set filled one id at a time, which is what the check was.

use std::collections::BTreeSet;

use katana_core::ErrorCode;
use katana_entity::{Entity, EntityId, Geometry, Model};
use katana_geometry::{Point2, Segment2};
use katana_storage::{ProjectContents, Relationship, apply_to_model};

fn line(id: EntityId) -> Entity {
    Entity {
        id,
        geometry: Geometry::Line(Segment2::new(
            Point2::new(0.0, 0.0),
            Point2::new(1.0, 1.0),
        )),
        ..Entity::default()
    }
}

fn contents_of(ids: &[EntityId]) -> ProjectContents {
    ProjectContents {
        entities: ids.iter().map(|&id| line(id)).collect(),
        next_entity_id: ids.iter().copied().max().unwrap_or(0) + 1,
        ..ProjectContents::default()
    }
}

/// The first id of `ids` that is zero or was met before, as an ordered set finds it.
fn first_refused(ids: &[EntityId]) -> Option<EntityId> {
    let mut seen = BTreeSet::new();
    ids.iter().copied().find(|&id| id == 0 || !seen.insert(id))
}

/// Knuth's MMIX linear congruential generator (The Art of Computer Programming, volume 2,
/// section 3.3.4, table 1, line 26): the same lists on every machine.
struct Sequence(u64);

impl Sequence {
    fn below(&mut self, below: u64) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) % below
    }
}

fn assert_judged_as_a_set_judges(ids: &[EntityId]) -> bool {
    let mut model = Model::default();
    let applied = apply_to_model(&contents_of(ids), &mut model);
    match first_refused(ids) {
        None => {
            assert!(applied.is_ok(), "{ids:?}: {applied:?}");
            let mut expected = ids.to_vec();
            expected.sort_unstable();
            assert_eq!(model.entities.ids(), expected, "{ids:?}");
            true
        }
        Some(id) => {
            let error = applied.expect_err("a missing or repeated id is refused");
            assert_eq!(error.code, ErrorCode::InvalidArgument, "{ids:?}");
            assert_eq!(error.message, "entity id is missing or duplicated", "{ids:?}");
            assert_eq!(error.context, format!("id={id}"), "{ids:?}");
            assert!(model.entities.empty(), "{ids:?}");
            false
        }
    }
}

#[test]
fn ids_in_hand_picked_orders_are_refused_exactly_where_a_set_refuses_them() {
    let lists: [&[EntityId]; 14] = [
        &[],
        &[1],
        &[1, 2, 3, 4],
        &[4, 3, 2, 1],
        &[5, 3, 9, 1, 7],
        // A repeat of the last, of an earlier ascending one, and of one that came out of order.
        &[1, 2, 2],
        &[1, 2, 3, 1],
        &[5, 3, 3],
        &[5, 3, 5],
        &[3, 5, 3],
        &[5, 3, 9, 3],
        // No id at all, first and later.
        &[0],
        &[1, 2, 0],
        // An id near the largest there is, after which nothing in the list ascends.
        &[7, EntityId::MAX - 1, 3, 7],
    ];
    for ids in lists {
        assert_judged_as_a_set_judges(ids);
    }
}

#[test]
fn generated_orders_with_and_without_repeats_are_judged_as_a_set_judges_them() {
    let mut sequence = Sequence(20_261_121);
    let (mut accepted, mut refused) = (0, 0);
    for round in 0..600 {
        let length = 1 + sequence.below(40) as usize;
        // Mostly ascending, as a project's are; every third list is shuffled by drawing its ids
        // at random, and ids are drawn from a range small enough that many lists repeat one.
        let mut ids: Vec<EntityId> = Vec::with_capacity(length);
        let mut next = 1;
        for _ in 0..length {
            if round % 3 == 0 {
                ids.push(1 + sequence.below(120));
            } else {
                next += sequence.below(3);
                ids.push(next);
                next += 1;
            }
        }
        if round % 3 == 1 && sequence.below(2) == 0 {
            // One id out of place in an ascending list: a repeat or a late arrival.
            let at = sequence.below(length as u64) as usize;
            ids[at] = 1 + sequence.below(120);
        }
        if assert_judged_as_a_set_judges(&ids) {
            accepted += 1;
        } else {
            refused += 1;
        }
    }
    // Both verdicts were reached many times, so neither path went untested.
    assert!(accepted > 50 && refused > 50, "{accepted} accepted, {refused} refused");
}

#[test]
fn a_relationship_is_checked_against_every_id_whatever_order_they_came_in() {
    // 3 and 1 arrive below the highest before them.
    let ids = [5, 3, 9, 1, 7];
    let relationship = |from, to| Relationship {
        from,
        to,
        kind: String::from("joins"),
        data: String::new(),
    };
    let mut sound = contents_of(&ids);
    sound.relationships = vec![relationship(5, 3), relationship(1, 9), relationship(7, 7)];
    let mut model = Model::default();
    assert!(apply_to_model(&sound, &mut model).is_ok());

    for (from, to) in [(5, 4), (2, 9), (10, 1), (0, 5)] {
        let mut dangling = contents_of(&ids);
        dangling.relationships = vec![relationship(from, to)];
        let mut untouched = Model::default();
        let error = apply_to_model(&dangling, &mut untouched)
            .expect_err("a relationship to an entity that is not there is refused");
        assert_eq!(error.code, ErrorCode::InvalidArgument, "{from} -> {to}");
        assert_eq!(
            error.message,
            "relationship refers to an entity that is not in the project"
        );
        assert_eq!(error.context, "kind=joins");
        assert!(untouched.entities.empty());
    }
}

#[test]
fn an_unknown_layer_or_style_is_named_with_the_entity_that_has_it() {
    let mut on_no_layer = contents_of(&[1, 2, 3]);
    on_no_layer.entities[1].layer = String::from("nowhere");
    let error = apply_to_model(&on_no_layer, &mut Model::default())
        .expect_err("an unknown layer is refused");
    assert_eq!(error.code, ErrorCode::InvalidArgument);
    assert_eq!(error.message, "entity refers to an unknown layer");
    assert_eq!(error.context, "id=2 layer=nowhere");

    let mut in_no_style = contents_of(&[1, 2, 3]);
    in_no_style.entities[2].style = String::from("nothing");
    let error = apply_to_model(&in_no_style, &mut Model::default())
        .expect_err("an unknown style is refused");
    assert_eq!(error.code, ErrorCode::InvalidArgument);
    assert_eq!(error.message, "entity refers to an unknown style");
    assert_eq!(error.context, "id=3 style=nothing");
}
