//! Applying a project's contents to the model, measured (CLAUDE.md section 5; the practice of
//! docs/testing.md, "Benchmarks"; docs/performance.md, "Checking a project's contents"): the part
//! of opening a project that comes after the database has been read - the contents checked, then
//! moved into a model built aside - and the same check a save makes before it writes.
//!
//! The contents are generated here: 100 000 lines with ids ascending from 1, as a project's are,
//! over 200 layers. No database is opened, so the cells time the check and the model alone. Only
//! the crate's public functions are used, so this source measures the base build and the changed
//! one alike. Cells, alternated round by round with the order reversed every round, each round
//! taking `REPEATS` samples of each:
//!
//! * `apply_consuming` and `apply_consuming_again`: `apply_to_model_consuming`, what
//!   `Document::open` calls, on a copy of the contents made before the clock starts, under two
//!   names, the A/A control;
//! * `apply_copying`: `apply_to_model`, which copies every record into the model;
//! * `refuse_last`: `apply_to_model` on contents whose last entity is on a layer the project
//!   lacks - the check alone, every entity passed before the refusal, and nothing applied.
//!
//! Prints one `cell=... min_ms=... median_ms=... samples=...` line a cell. Run in Release:
//! `cargo bench --locked -p katana_storage --bench apply_contents` (or through
//! `cargo test --release`, which runs it too, one round); `KATANA_BENCH_ROUNDS` sets the rounds
//! (default 7).

use std::hint::black_box;
use std::time::Instant;

use katana_entity::{Entity, EntityId, Geometry, Layer, Model};
use katana_geometry::{Point2, Segment2};
use katana_storage::{ProjectContents, apply_to_model, apply_to_model_consuming};

const ENTITIES: usize = 100_000;
const LAYERS: usize = 200;
const REPEATS: usize = 3;

fn contents() -> ProjectContents {
    let layers: Vec<Layer> = (0..LAYERS)
        .map(|layer| Layer {
            // Root layers: a sublayer's parents would have to be listed too.
            name: format!("code{layer:03}"),
            ..Layer::default()
        })
        .collect();
    let entities = (0..ENTITIES)
        .map(|index| {
            let (x, y) = ((index % 400) as f64 * 2.5, (index / 400) as f64 * 2.5);
            Entity {
                id: index as EntityId + 1,
                geometry: Geometry::Line(Segment2::new(
                    Point2::new(x, y),
                    Point2::new(x + 2.0, y + 1.0),
                )),
                // Runs of 25 on a layer, as a survey's strings come.
                layer: layers[(index / 25 * 7) % LAYERS].name.clone(),
                ..Entity::default()
            }
        })
        .collect();
    ProjectContents {
        layers,
        entities,
        next_entity_id: ENTITIES as EntityId + 1,
        ..ProjectContents::default()
    }
}

/// One sample of `cell`, in milliseconds.
fn sample(cell: &str, sound: &ProjectContents, unsound: &ProjectContents) -> f64 {
    let mut model = Model::default();
    let started;
    match cell {
        "apply_copying" => {
            started = Instant::now();
            apply_to_model(black_box(sound), &mut model).expect("sound contents are applied");
        }
        "refuse_last" => {
            started = Instant::now();
            let refused = apply_to_model(black_box(unsound), &mut model);
            assert!(refused.is_err());
        }
        _ => {
            let copy = sound.clone();
            started = Instant::now();
            apply_to_model_consuming(black_box(copy), &mut model)
                .expect("sound contents are applied");
        }
    }
    let taken = started.elapsed().as_secs_f64() * 1000.0;
    black_box(model.entities.size());
    taken
}

fn main() {
    let rounds: usize = std::env::var("KATANA_BENCH_ROUNDS")
        .ok()
        .and_then(|text| text.parse().ok())
        .unwrap_or(7);
    // `cargo test --release` runs a bench target as a test with `--bench` absent: once, quickly.
    let quick = !std::env::args().any(|argument| argument == "--bench");
    let rounds = if quick { 1 } else { rounds.max(1) };
    let sound = contents();
    let mut unsound = sound.clone();
    if let Some(last) = unsound.entities.last_mut() {
        last.layer = String::from("a/layer/the/project/lacks");
    }
    let mut cells = [
        "apply_consuming",
        "apply_copying",
        "refuse_last",
        "apply_consuming_again",
    ];
    let mut samples: Vec<(&str, Vec<f64>)> = cells.iter().map(|cell| (*cell, Vec::new())).collect();
    for round in 0..rounds {
        if round % 2 == 1 {
            cells.reverse();
        }
        for cell in cells {
            for _ in 0..REPEATS {
                let taken = sample(cell, &sound, &unsound);
                if let Some((_, kept)) = samples.iter_mut().find(|(name, _)| *name == cell) {
                    kept.push(taken);
                }
            }
        }
        if round % 2 == 1 {
            cells.reverse();
        }
    }
    println!("entities={ENTITIES} layers={LAYERS}");
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
