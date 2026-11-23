//! A project saved and loaded through its database, measured (CLAUDE.md section 5; the practice
//! of docs/testing.md, "Benchmarks"; docs/performance.md, "A project saved and loaded"): what
//! opening a drawing pays to read its rows, and what a save pays to write them - the part of an
//! open that `apply_contents` leaves out, which times the check and the model alone.
//!
//! The project is generated here: 100 000 entities with ids ascending from 1 over 200 layers, in
//! runs of 25 on a layer - lines and polylines of 12 vertices by turns, and one in fifty a
//! surveyed point that carries three properties, so nearly every entity's two property columns
//! hold the empty object, as a real drawing's do. It is saved once to a project in a folder of
//! this run's own under the system's temporary directory, which is removed when the run ends
//! (and before it starts, should an earlier run have been killed). Only the crate's public
//! members are used, so this source measures the base build and the changed one alike. Cells,
//! alternated round by round with the order reversed every round, each round taking `REPEATS`
//! samples of each:
//!
//! * `load` and `load_again`: `ProjectStore::load` on a store opened before the clock starts -
//!   every row read into a `ProjectContents` - under two names, the A/A control;
//! * `open_load_apply`: `ProjectStore::open` (its integrity check included), `load` and
//!   `apply_to_model_consuming`: what `Document::open` pays before it builds its index;
//! * `save`: `ProjectStore::save` of the same contents over the project, as a save of an open
//!   drawing rewrites it.
//!
//! The times are the disk's as much as the code's: read the A/A pair first. Prints one
//! `cell=... min_ms=... median_ms=... samples=...` line a cell. Run in Release:
//! `cargo bench --locked -p katana_storage --bench project_load` (or through
//! `cargo test --release`, which runs it too, one round); `KATANA_BENCH_ROUNDS` sets the rounds
//! (default 7).

use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::time::Instant;

use katana_entity::{Entity, EntityId, Geometry, Layer, Model, PointGeometry, PropertyValue};
use katana_geometry::{Point2, Polyline2, Segment2};
use katana_storage::{ProjectContents, ProjectMetadata, ProjectStore, apply_to_model_consuming};

const ENTITIES: usize = 100_000;
const LAYERS: usize = 200;
const REPEATS: usize = 3;

/// The run's own folder, removed when the run ends however it ends.
struct Scratch {
    root: PathBuf,
}

impl Scratch {
    fn new() -> Self {
        // The process id: two runs at once (the base build's and the changed one's) must not
        // share a project.
        let root = std::env::temp_dir().join(format!(
            "katana-storage-bench-project-load-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("the scratch folder is made");
        Self { root }
    }

    fn project(&self) -> PathBuf {
        self.root.join("drawing.katana")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn contents() -> ProjectContents {
    // The default layer first, then root layers: a sublayer's parents would have to be listed
    // too.
    let mut layers = vec![Layer::default()];
    layers.extend((0..LAYERS).map(|layer| Layer {
        name: format!("code{layer:03}"),
        ..Layer::default()
    }));
    let entities = (0..ENTITIES)
        .map(|index| {
            let (x, y) = ((index % 400) as f64 * 2.5, (index / 400) as f64 * 2.5);
            let mut entity = Entity {
                id: index as EntityId + 1,
                // Runs of 25 on a layer, as a survey's strings come.
                layer: format!("code{:03}", (index / 25 * 7) % LAYERS),
                ..Entity::default()
            };
            if index.is_multiple_of(50) {
                entity.geometry = Geometry::Point(PointGeometry {
                    position: Point2::new(x, y),
                });
                entity.properties.insert(
                    String::from("code"),
                    PropertyValue::from(format!("KB{:03}", index / 200)),
                );
                entity.properties.insert(
                    String::from("point"),
                    PropertyValue::from(format!("{}", index / 50 + 1)),
                );
                entity.properties.insert(
                    String::from("height"),
                    PropertyValue::from(20.0 + (index % 97) as f64 * 0.125),
                );
            } else if index.is_multiple_of(2) {
                entity.geometry = Geometry::Line(Segment2::new(
                    Point2::new(x, y),
                    Point2::new(x + 2.0, y + 1.0),
                ));
            } else {
                let vertices = (0..12)
                    .map(|vertex| {
                        Point2::new(x + f64::from(vertex) * 0.2, y + f64::from(vertex % 2) * 0.8)
                    })
                    .collect();
                entity.geometry = Geometry::Polyline(Polyline2::new(vertices, false));
            }
            entity
        })
        .collect();
    ProjectContents {
        layers,
        entities,
        next_entity_id: ENTITIES as EntityId + 1,
        ..ProjectContents::default()
    }
}

/// One sample of `cell`, in milliseconds. Every store a sample opens is closed before it
/// returns: Windows will not remove a database that is open.
fn sample(cell: &str, project: &Path, contents: &ProjectContents) -> f64 {
    let started;
    match cell {
        "open_load_apply" => {
            let mut model = Model::default();
            started = Instant::now();
            let store = ProjectStore::open(black_box(project)).expect("the project opens");
            let loaded = store.load().expect("the project loads");
            apply_to_model_consuming(loaded, &mut model).expect("the contents are applied");
            assert_eq!(black_box(model.entities.size()), ENTITIES);
        }
        "save" => {
            let mut store = ProjectStore::open(project).expect("the project opens");
            started = Instant::now();
            store
                .save(black_box(contents))
                .expect("the project is saved");
        }
        _ => {
            let store = ProjectStore::open(project).expect("the project opens");
            started = Instant::now();
            let loaded = black_box(&store).load().expect("the project loads");
            assert_eq!(black_box(loaded.entities.len()), ENTITIES);
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
    let scratch = Scratch::new();
    let project = scratch.project();
    let contents = contents();
    let database = {
        let mut store = ProjectStore::create(&project, &ProjectMetadata::default())
            .expect("the project is created");
        store.save(&contents).expect("the project is saved");
        store.database_path()
    }; // closed: each sample opens its own
    let mut cells = ["load", "open_load_apply", "save", "load_again"];
    let mut samples: Vec<(&str, Vec<f64>)> = cells.iter().map(|cell| (*cell, Vec::new())).collect();
    for round in 0..rounds {
        if round % 2 == 1 {
            cells.reverse();
        }
        for cell in cells {
            for _ in 0..REPEATS {
                let taken = sample(cell, &project, &contents);
                if let Some((_, kept)) = samples.iter_mut().find(|(name, _)| *name == cell) {
                    kept.push(taken);
                }
            }
        }
        if round % 2 == 1 {
            cells.reverse();
        }
    }
    let bytes = std::fs::metadata(&database).map_or(0, |file| file.len());
    println!("entities={ENTITIES} layers={LAYERS} database_bytes={bytes}");
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
