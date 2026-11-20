//! The entity database's kept layer counts against the walk they replaced
//! (docs/performance.md, "Layer counts"; CLAUDE.md section 5: a fast path is
//! held to the exhaustive one by a test).
//!
//! `count_on_layer` answers from counts kept in step by add, insert, replace,
//! remove, clear and adopt_contents; `ids_on_layer` still walks every entity.
//! After every step of a generated sequence of all six, each layer's count is
//! the length of its walk.

use katana_core::error::ErrorCode;
use katana_entity::{Entity, EntityDatabase, EntityId, Geometry};
use katana_geometry::{Point2, Segment2};

/// Layers the sequence draws on, and one it never does.
const LAYERS: [&str; 5] = ["0", "survey", "survey/kerb", "design/road/centre", "Survey"];
const UNUSED: &str = "never/used";

fn line_on(layer: &str) -> Entity {
    Entity {
        geometry: Geometry::Line(Segment2::new(
            Point2::new(0.0, 0.0),
            Point2::new(1.0, 1.0),
        )),
        layer: String::from(layer),
        ..Entity::default()
    }
}

/// Knuth's MMIX linear congruential generator (The Art of Computer
/// Programming, volume 2, section 3.3.4, table 1, line 26): the sequence is
/// the same on every machine.
struct Sequence(u64);

impl Sequence {
    fn below(&mut self, below: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((self.0 >> 33) as usize) % below
    }
}

fn assert_counts_are_the_walks(database: &EntityDatabase, step: usize) {
    let mut total = 0;
    for layer in LAYERS.iter().copied().chain([UNUSED, ""]) {
        let walked = database.ids_on_layer(layer).len();
        assert_eq!(
            database.count_on_layer(layer),
            walked,
            "layer {layer:?} after step {step}"
        );
        total += walked;
    }
    // Every entity is on one of the layers asked about, so nothing was counted
    // on a name the loop above did not ask.
    assert_eq!(total, database.size(), "after step {step}");
}

#[test]
fn every_layer_count_is_the_length_of_the_walk_after_every_kind_of_change() {
    let mut sequence = Sequence(20_261_121);
    let mut database = EntityDatabase::new();
    let mut live: Vec<EntityId> = Vec::new();
    let mut kinds = [0usize; 6];
    for step in 0..4_000 {
        let layer = LAYERS[sequence.below(LAYERS.len())];
        let kind = match sequence.below(100) {
            0..40 => 0,
            40..50 => 1,
            50..75 => 2,
            75..98 => 3,
            98 => 4,
            _ => 5,
        };
        match kind {
            0 => {
                live.push(database.add(line_on(layer)).expect("a line is added"));
            }
            // Under an id of its own, as undo of a removal and a load do.
            1 => {
                let id = database.next_id() + 1 + sequence.below(3) as EntityId;
                let mut entity = line_on(layer);
                entity.id = id;
                database.insert(entity).expect("a free id is taken");
                live.push(id);
            }
            // Onto another layer, or the same one.
            2 if !live.is_empty() => {
                let id = live[sequence.below(live.len())];
                let mut entity = line_on(layer);
                entity.id = id;
                database.replace(entity).expect("a stored entity is replaced");
            }
            3 if !live.is_empty() => {
                let id = live.swap_remove(sequence.below(live.len()));
                database.remove(id).expect("a stored entity is removed");
            }
            4 => {
                database.clear();
                live.clear();
            }
            // A load: contents built aside and adopted, their counts with them.
            5 => {
                let mut loaded = EntityDatabase::new();
                live.clear();
                for _ in 0..sequence.below(40) {
                    let on = LAYERS[sequence.below(LAYERS.len())];
                    live.push(loaded.add(line_on(on)).expect("a line is added"));
                }
                database.adopt_contents(loaded);
            }
            _ => continue,
        }
        kinds[kind] += 1;
        assert_counts_are_the_walks(&database, step);
    }
    // The sequence reached every kind of change, each many times but the two
    // rare ones, which it reached more than once.
    assert!(kinds[..4].iter().all(|&count| count > 100), "{kinds:?}");
    assert!(kinds[4] > 1 && kinds[5] > 1, "{kinds:?}");
}

#[test]
fn a_refused_change_leaves_the_counts_as_they_were() {
    let mut database = EntityDatabase::new();
    let id = database.add(line_on("survey")).expect("a line is added");

    // No layer name: refused by add, insert and replace alike.
    assert_eq!(
        database.add(line_on("")).expect_err("no layer").code,
        ErrorCode::InvalidArgument
    );
    let mut unnamed = line_on("");
    unnamed.id = id;
    assert_eq!(
        database.replace(unnamed).expect_err("no layer").code,
        ErrorCode::InvalidArgument
    );
    // An id that is taken, and one that is not there.
    let mut taken = line_on("design/road/centre");
    taken.id = id;
    assert_eq!(
        database.insert(taken).expect_err("the id is taken").code,
        ErrorCode::AlreadyExists
    );
    let mut absent = line_on("design/road/centre");
    absent.id = id + 10;
    assert_eq!(
        database.replace(absent).expect_err("no such entity").code,
        ErrorCode::NotFound
    );
    assert_eq!(
        database.remove(id + 10).expect_err("no such entity").code,
        ErrorCode::NotFound
    );

    assert_eq!(database.count_on_layer("survey"), 1);
    assert_eq!(database.count_on_layer("design/road/centre"), 0);
    assert_eq!(database.count_on_layer(""), 0);
    assert_counts_are_the_walks(&database, 0);
}

#[test]
fn a_copy_of_the_database_counts_for_itself() {
    let mut database = EntityDatabase::new();
    let kept = database.add(line_on("survey")).expect("a line is added");
    let mut copy = database.clone();
    copy.remove(kept).expect("the copy's entity is removed");
    copy.add(line_on("survey/kerb")).expect("a line is added");

    assert_eq!(database.count_on_layer("survey"), 1);
    assert_eq!(database.count_on_layer("survey/kerb"), 0);
    assert_eq!(copy.count_on_layer("survey"), 0);
    assert_eq!(copy.count_on_layer("survey/kerb"), 1);
}
