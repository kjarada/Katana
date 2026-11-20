//! `EntityDatabase::iter` against the lookups it replaces (docs/performance.md, "The labels'
//! walk"; CLAUDE.md section 5): one pass over the entities must hand out exactly the entities
//! `ids()` names and `find()` returns, in that order, so a caller moved from one to the other
//! keeps what it kept.

use katana_entity::{Entity, EntityDatabase, EntityId, Geometry, PointGeometry};
use katana_geometry::Point2;

fn point(x: f64, layer: &str) -> Entity {
    Entity {
        geometry: Geometry::Point(PointGeometry {
            position: Point2::new(x, -x),
        }),
        layer: String::from(layer),
        ..Entity::default()
    }
}

#[test]
fn the_walk_hands_out_what_each_id_finds_in_ascending_id_order() {
    let mut database = EntityDatabase::new();
    let mut added: Vec<EntityId> = Vec::new();
    for index in 0..200 {
        let layer = if index % 3 == 0 { "survey" } else { "0" };
        added.push(
            database
                .add(point(f64::from(index), layer))
                .expect("a point is added"),
        );
    }
    // Gaps in the ids, an id far above the rest put in out of turn, and one replaced.
    for id in added.iter().copied().filter(|id| id % 7 == 0) {
        database.remove(id).expect("a stored point is removed");
    }
    let mut late = point(1.0e6, "survey");
    late.id = 5_000;
    database.insert(late).expect("a free id is taken");
    database
        .add(point(2.0e6, "0"))
        .expect("a point is added after it");
    let mut moved = point(-4.0, "survey");
    moved.id = added[3];
    database.replace(moved).expect("a stored point is replaced");

    let walked: Vec<&Entity> = database.iter().collect();
    let looked_up: Vec<&Entity> = database
        .ids()
        .into_iter()
        .map(|id| database.find(id).expect("an id the database lists"))
        .collect();
    assert_eq!(walked.len(), database.size());
    assert_eq!(walked, looked_up);
    assert!(walked.windows(2).all(|pair| pair[0].id < pair[1].id));
    // The very entities, not copies of them.
    assert!(
        walked
            .iter()
            .zip(&looked_up)
            .all(|(a, b)| std::ptr::eq(*a, *b))
    );
}

#[test]
fn an_empty_database_walks_nothing() {
    assert_eq!(EntityDatabase::new().iter().count(), 0);
    let mut database = EntityDatabase::new();
    database.add(point(1.0, "0")).expect("a point is added");
    database.clear();
    assert_eq!(database.iter().count(), 0);
}
