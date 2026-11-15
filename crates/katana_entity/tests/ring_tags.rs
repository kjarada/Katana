//! The twin of tests/entity/test_ring_tags.cpp: rings that are holes - how an
//! entity says it is one, and which exterior a hole joins.

use katana_entity::ring_tags::{is_tagged_hole, ring_part};
use katana_entity::{Entity, EntityId, ExteriorCandidate, HoleJoiner, PropertyValue};
use katana_geometry::{Point2, Polyline2};

fn p(x: f64, y: f64) -> Point2 {
    Point2::new(x, y)
}

/// The square [x, x + side] x [y, y + side] as a closed ring that does not
/// repeat its first vertex.
fn square(x: f64, y: f64, side: f64) -> Polyline2 {
    Polyline2::new(
        vec![
            p(x, y),
            p(x + side, y),
            p(x + side, y + side),
            p(x, y + side),
        ],
        true,
    )
}

fn exterior(id: EntityId, x: f64, y: f64, side: f64, part: Option<&str>) -> ExteriorCandidate {
    ExteriorCandidate {
        id,
        part: part.map(String::from),
        ring: square(x, y, side),
    }
}

fn some(text: &str) -> Option<String> {
    Some(String::from(text))
}

mod ring_tags {
    use super::*;

    #[test]
    fn an_entity_is_a_hole_only_when_a_ring_tag_says_hole_in_any_case_in_its_properties_or_metadata()
     {
        let untagged = Entity::default();
        assert!(!is_tagged_hole(&untagged));

        let mut imported = Entity::default(); // IMPORT writes metadata
        imported
            .metadata
            .insert(String::from("source.ring"), PropertyValue::from("hole"));
        assert!(is_tagged_hole(&imported));
        imported
            .metadata
            .insert(String::from("source.ring"), PropertyValue::from("HOLE"));
        assert!(is_tagged_hole(&imported));

        let mut written = Entity::default(); // a result writes properties
        written
            .properties
            .insert(String::from("gis.ring"), PropertyValue::from("Hole"));
        assert!(is_tagged_hole(&written));

        // An exterior says so, and a word that merely contains the word is not
        // it.
        let mut outer = Entity::default();
        outer
            .metadata
            .insert(String::from("source.ring"), PropertyValue::from("exterior"));
        assert!(!is_tagged_hole(&outer));
        outer.metadata.insert(
            String::from("source.ring"),
            PropertyValue::from("not a hole"),
        );
        assert!(!is_tagged_hole(&outer));
        // One tag saying hole is enough, though the other says exterior.
        outer
            .properties
            .insert(String::from("gis.ring"), PropertyValue::from("hole"));
        assert!(is_tagged_hole(&outer));
    }

    #[test]
    fn the_part_is_the_results_tag_before_the_imports_and_none_when_neither_is_there() {
        let none = Entity::default();
        assert!(ring_part(&none).is_none());

        let mut imported = Entity::default();
        imported.metadata.insert(
            String::from("source.part"),
            PropertyValue::from("feature 3"),
        );
        assert_eq!(ring_part(&imported), some("feature 3"));

        // A result writes the exterior's id as an integer; it reads as its text.
        let mut written = Entity::default();
        written
            .properties
            .insert(String::from("gis.part"), PropertyValue::Integer(7));
        written.metadata.insert(
            String::from("source.part"),
            PropertyValue::from("feature 3"),
        );
        assert_eq!(ring_part(&written), some("7"));
    }
}

mod hole_joiner {
    use super::*;

    #[test]
    fn a_hole_joins_the_ring_that_holds_all_of_it_and_none_when_one_vertex_is_outside() {
        let joiner = HoleJoiner::new(vec![
            exterior(1, 0.0, 0.0, 100.0, None),
            exterior(2, 200.0, 0.0, 100.0, None),
        ]);
        // [40, 60]^2 lies in the first.
        let inside = [p(40.0, 40.0), p(60.0, 40.0), p(60.0, 60.0), p(40.0, 60.0)];
        assert_eq!(joiner.exterior_of(&inside, 9, &None), Some(0));
        // In the second, by its position, not its order.
        let second = [
            p(240.0, 40.0),
            p(260.0, 40.0),
            p(260.0, 60.0),
            p(240.0, 60.0),
        ];
        assert_eq!(joiner.exterior_of(&second, 9, &None), Some(1));
        // One vertex out of the first (x = 101): held by no ring.
        let straddling = [p(40.0, 40.0), p(101.0, 40.0), p(60.0, 60.0)];
        assert!(joiner.exterior_of(&straddling, 9, &None).is_none());
        // Between the two rings.
        let between = [p(120.0, 40.0), p(180.0, 40.0), p(150.0, 60.0)];
        assert!(joiner.exterior_of(&between, 9, &None).is_none());
        // A vertex on an edge is held, as Polyline2::contains says: a hole that
        // shares an edge with its exterior is still its hole.
        let on_the_edge = [p(0.0, 40.0), p(20.0, 40.0), p(20.0, 60.0), p(0.0, 60.0)];
        assert_eq!(joiner.exterior_of(&on_the_edge, 9, &None), Some(0));
        // No exterior to join.
        assert!(
            HoleJoiner::new(Vec::new())
                .exterior_of(&inside, 9, &None)
                .is_none()
        );
    }

    #[test]
    fn of_several_rings_that_hold_a_hole_it_joins_the_smallest_unless_it_shares_a_part_with_another()
     {
        // A lot of 100 x 100 (10 000 m2) with a smaller one of 60 x 60 (3600 m2)
        // inside it, and a hole at [40, 50]^2 inside both.
        let hole = [p(40.0, 40.0), p(50.0, 40.0), p(50.0, 50.0), p(40.0, 50.0)];
        {
            let joiner = HoleJoiner::new(vec![
                exterior(1, 0.0, 0.0, 100.0, None),
                exterior(2, 20.0, 20.0, 60.0, None),
            ]);
            assert_eq!(
                joiner.exterior_of(&hole, 9, &None),
                Some(1),
                "the smaller of the two"
            );
        }
        {
            // The hole's own feature is the big one's: that is the one it
            // belongs to, though the small is smaller.
            let joiner = HoleJoiner::new(vec![
                exterior(1, 0.0, 0.0, 100.0, Some("feature 1")),
                exterior(2, 20.0, 20.0, 60.0, Some("feature 2")),
            ]);
            assert_eq!(joiner.exterior_of(&hole, 9, &some("feature 1")), Some(0));
            assert_eq!(joiner.exterior_of(&hole, 9, &some("feature 2")), Some(1));
            // A part that matches no ring changes nothing: the smallest.
            assert_eq!(joiner.exterior_of(&hole, 9, &some("feature 7")), Some(1));
        }
        {
            // A part shared with a ring that does not hold the hole does not
            // count.
            let joiner = HoleJoiner::new(vec![
                exterior(1, 0.0, 0.0, 100.0, Some("feature 1")),
                exterior(2, 300.0, 0.0, 60.0, Some("feature 2")),
            ]);
            assert_eq!(joiner.exterior_of(&hole, 9, &some("feature 2")), Some(0));
        }
    }

    #[test]
    fn of_equal_rings_the_hole_joins_the_one_made_nearest_before_it() {
        // The same file imported twice: two exteriors equal in everything but
        // their ids (1 and 5), each hole made just after its own (6, and 2).
        let hole = [p(40.0, 40.0), p(50.0, 40.0), p(50.0, 50.0), p(40.0, 50.0)];
        let joiner = HoleJoiner::new(vec![
            exterior(1, 0.0, 0.0, 100.0, None),
            exterior(5, 0.0, 0.0, 100.0, None),
        ]);
        // Hole 6: exterior 5 is one before it, exterior 1 five.
        assert_eq!(joiner.exterior_of(&hole, 6, &None), Some(1));
        // Hole 2: exterior 1 is one before it, and 5 is after it, which is the
        // farthest there is.
        assert_eq!(joiner.exterior_of(&hole, 2, &None), Some(0));
        // The order the rings are given in does not matter.
        let reversed = HoleJoiner::new(vec![
            exterior(5, 0.0, 0.0, 100.0, None),
            exterior(1, 0.0, 0.0, 100.0, None),
        ]);
        assert_eq!(reversed.exterior_of(&hole, 6, &None), Some(0));
        assert_eq!(reversed.exterior_of(&hole, 2, &None), Some(1));
    }
}
