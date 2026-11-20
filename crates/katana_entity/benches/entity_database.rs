//! The entity database's layer counts, measured (CLAUDE.md section 5; the practice of
//! docs/testing.md, "Benchmarks"; docs/performance.md, "Layer counts"): what the layers panel,
//! the layer manager and LAYER LIST pay to say how many entities each layer holds, and what
//! keeping the counts costs the changes that must keep them.
//!
//! The drawing is generated here: 100 000 lines over 200 layers named as a survey's are
//! ("survey/code017"), each line on the layer its index picks. Only the database's public
//! members are used, so this source measures the base build and the changed one alike. Cells,
//! alternated round by round with the order reversed every round, each round taking `REPEATS`
//! samples of each:
//!
//! * `count_every_layer` and `count_every_layer_again`: `count_on_layer` once for each of the 200
//!   layers - one frame of a layers panel showing them all - under two names, the A/A control;
//! * `add_entities`: the 100 000 lines added to an empty database (a load, an import);
//! * `replace_onto_another_layer`: 10 000 of them replaced by a copy on another layer (a Global
//!   Modify of a tenth of the drawing);
//! * `remove_entities`: 10 000 of them removed.
//!
//! Prints one `cell=... min_ms=... median_ms=... samples=...` line a cell. Run in Release:
//! `cargo bench --locked -p katana_entity --bench entity_database` (or through
//! `cargo test --release`, which runs it too, one round); `KATANA_BENCH_ROUNDS` sets the rounds
//! (default 7).

use std::hint::black_box;
use std::time::Instant;

use katana_entity::{Entity, EntityDatabase, EntityId, Geometry};
use katana_geometry::{Point2, Segment2};

const ENTITIES: usize = 100_000;
const LAYERS: usize = 200;
const CHANGED: usize = 10_000;
const REPEATS: usize = 3;

fn layer_names() -> Vec<String> {
    (0..LAYERS)
        .map(|layer| format!("survey/code{layer:03}"))
        .collect()
}

fn line(index: usize, layer: &str) -> Entity {
    let (x, y) = ((index % 400) as f64 * 2.5, (index / 400) as f64 * 2.5);
    Entity {
        geometry: Geometry::Line(Segment2::new(
            Point2::new(x, y),
            Point2::new(x + 2.0, y + 1.0),
        )),
        layer: String::from(layer),
        ..Entity::default()
    }
}

/// The layer of line `index`: runs of 25 on a layer, as a survey's strings come, the runs
/// spread over every layer.
fn layer_of(index: usize) -> usize {
    (index / 25 * 7) % LAYERS
}

fn drawing(layers: &[String]) -> EntityDatabase {
    let mut database = EntityDatabase::new();
    for index in 0..ENTITIES {
        database
            .add(line(index, &layers[layer_of(index)]))
            .expect("a line is added");
    }
    database
}

/// One sample of `cell`, in milliseconds. A cell that changes the database works on a copy made
/// before the clock starts.
fn sample(cell: &str, layers: &[String], database: &EntityDatabase) -> f64 {
    let started;
    match cell {
        "add_entities" => {
            let lines: Vec<Entity> = (0..ENTITIES)
                .map(|index| line(index, &layers[layer_of(index)]))
                .collect();
            started = Instant::now();
            let mut built = EntityDatabase::new();
            for entity in lines {
                built.add(entity).expect("a line is added");
            }
            black_box(built.size());
        }
        "replace_onto_another_layer" => {
            let mut copy = database.clone();
            let moved: Vec<Entity> = (0..CHANGED)
                .map(|step| {
                    let index = step * (ENTITIES / CHANGED);
                    let mut entity = line(index, &layers[(layer_of(index) + 1) % LAYERS]);
                    entity.id = index as EntityId + 1;
                    entity
                })
                .collect();
            started = Instant::now();
            for entity in moved {
                copy.replace(entity).expect("a stored line is replaced");
            }
            black_box(copy.size());
        }
        "remove_entities" => {
            let mut copy = database.clone();
            started = Instant::now();
            for step in 0..CHANGED {
                let id = (step * (ENTITIES / CHANGED)) as EntityId + 1;
                black_box(copy.remove(id).expect("a stored line is removed"));
            }
        }
        _ => {
            started = Instant::now();
            let mut total = 0;
            for layer in layers {
                total += database.count_on_layer(black_box(layer));
            }
            assert_eq!(black_box(total), ENTITIES);
        }
    }
    started.elapsed().as_secs_f64() * 1000.0
}

fn main() {
    let rounds: usize = std::env::var("KATANA_BENCH_ROUNDS")
        .ok()
        .and_then(|text| text.parse().ok())
        .unwrap_or(7);
    // `cargo test --release` runs a bench target as a test with `--bench` absent: once, quickly.
    let quick = !std::env::args().any(|argument| argument == "--bench");
    let rounds = if quick { 1 } else { rounds.max(1) };
    let layers = layer_names();
    let database = drawing(&layers);
    let mut cells = [
        "count_every_layer",
        "add_entities",
        "replace_onto_another_layer",
        "remove_entities",
        "count_every_layer_again",
    ];
    let mut samples: Vec<(&str, Vec<f64>)> = cells.iter().map(|cell| (*cell, Vec::new())).collect();
    for round in 0..rounds {
        if round % 2 == 1 {
            cells.reverse();
        }
        for cell in cells {
            for _ in 0..REPEATS {
                let taken = sample(cell, &layers, &database);
                if let Some((_, kept)) = samples.iter_mut().find(|(name, _)| *name == cell) {
                    kept.push(taken);
                }
            }
        }
        if round % 2 == 1 {
            cells.reverse();
        }
    }
    println!("entities={ENTITIES} layers={LAYERS} changed={CHANGED}");
    for (cell, mut kept) in samples {
        kept.sort_by(f64::total_cmp);
        let median = kept[kept.len() / 2];
        println!(
            "cell={cell} min_ms={:.3} median_ms={:.3} samples={}",
            kept[0],
            median,
            kept.len()
        );
    }
}
