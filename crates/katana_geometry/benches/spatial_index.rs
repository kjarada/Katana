//! The spatial index, measured (CLAUDE.md section 5; the practice of docs/testing.md,
//! "Benchmarks"; docs/performance.md, "The spatial index's hashing"): what opening or importing a
//! drawing pays to build it, what a repaint, a pick and a snap pay to ask it, and what a command
//! pays to keep it.
//!
//! The boxes are generated here: 100 000 of them, each up to 6 by 6 units, scattered over a site
//! of 2 000 by 1 250 units, as the strings and points of a large survey lie. Only the index's
//! public members are used, so this source measures the base build and the changed one alike.
//! Cells, alternated round by round with the order reversed every round, each round taking
//! `REPEATS` samples of each:
//!
//! * `query_view` and `query_view_again`: 200 queries of a window a hundredth of the site's area
//!   (a zoomed-in repaint; about 1 000 boxes each), under two names, the A/A control;
//! * `query_cursor`: 20 000 queries of a window 2 units square (a snap or a pick at the cursor);
//! * `rebuild`: the whole index built from the 100 000 boxes (an open, an import);
//! * `move_boxes`: 10 000 boxes each moved by a unit through `insert` (a Move command), and back.
//!
//! Prints one `cell=... min_ms=... median_ms=... samples=...` line a cell, and the number of ids
//! the queries returned, which must be the same in both builds. Run in Release:
//! `cargo bench --locked -p katana_geometry --bench spatial_index` (or through
//! `cargo test --release`, which runs it too, one round); `KATANA_BENCH_ROUNDS` sets the rounds
//! (default 7).

use std::hint::black_box;
use std::time::Instant;

use katana_geometry::{Box2, Point2, SpatialEntry, SpatialId, SpatialIndex};

const BOXES: usize = 100_000;
const SITE_WIDTH: f64 = 2_000.0;
const SITE_HEIGHT: f64 = 1_250.0;
const VIEW_QUERIES: usize = 200;
const CURSOR_QUERIES: usize = 20_000;
const MOVED: usize = 10_000;
const REPEATS: usize = 3;

/// Knuth's MMIX linear congruential generator (The Art of Computer Programming, volume 2,
/// section 3.3.4, table 1, line 26), as a real in [0, 1): the same boxes on every machine.
struct Sequence(u64);

impl Sequence {
    fn real(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 11) as f64 * (1.0 / 9_007_199_254_740_992.0)
    }
}

fn window(east: f64, north: f64, width: f64, height: f64) -> Box2 {
    Box2::new(
        Point2::new(east, north),
        Point2::new(east + width, north + height),
    )
}

fn boxes() -> Vec<SpatialEntry> {
    let mut sequence = Sequence(2026);
    (0..BOXES)
        .map(|index| SpatialEntry {
            id: index as SpatialId + 1,
            box_value: window(
                sequence.real() * SITE_WIDTH,
                sequence.real() * SITE_HEIGHT,
                0.05 + sequence.real() * 6.0,
                0.05 + sequence.real() * 6.0,
            ),
        })
        .collect()
}

fn windows(count: usize, width: f64, height: f64, seed: u64) -> Vec<Box2> {
    let mut sequence = Sequence(seed);
    (0..count)
        .map(|_| {
            window(
                sequence.real() * (SITE_WIDTH - width),
                sequence.real() * (SITE_HEIGHT - height),
                width,
                height,
            )
        })
        .collect()
}

struct Inputs {
    entries: Vec<SpatialEntry>,
    index: SpatialIndex,
    views: Vec<Box2>,
    cursors: Vec<Box2>,
}

/// One sample of `cell`: its milliseconds, and how many ids its queries returned (0 for a cell
/// that asks nothing).
fn sample(cell: &str, inputs: &Inputs) -> (f64, usize) {
    let mut found = 0;
    let started;
    match cell {
        "rebuild" => {
            let mut index = SpatialIndex::default();
            started = Instant::now();
            index.rebuild(black_box(&inputs.entries));
            black_box(index.size());
        }
        "move_boxes" => {
            let mut index = inputs.index.clone();
            let step = BOXES / MOVED;
            started = Instant::now();
            for shift in [1.0, 0.0] {
                for entry in inputs.entries.iter().step_by(step) {
                    let moved = Box2::new(
                        Point2::new(entry.box_value.min.x + shift, entry.box_value.min.y),
                        Point2::new(entry.box_value.max.x + shift, entry.box_value.max.y),
                    );
                    index.insert(entry.id, moved);
                }
            }
            black_box(index.size());
        }
        _ => {
            let queries = if cell == "query_cursor" {
                &inputs.cursors
            } else {
                &inputs.views
            };
            // The caller's buffer, kept between queries as a mouse-move path keeps its own.
            let mut ids: Vec<SpatialId> = Vec::new();
            started = Instant::now();
            for query in queries {
                inputs.index.query_into(black_box(*query), &mut ids);
                found += ids.len();
            }
            black_box(found);
        }
    }
    (started.elapsed().as_secs_f64() * 1000.0, found)
}

fn main() {
    let rounds: usize = std::env::var("KATANA_BENCH_ROUNDS")
        .ok()
        .and_then(|text| text.parse().ok())
        .unwrap_or(7);
    // `cargo test --release` runs a bench target as a test with `--bench` absent: once, quickly.
    let quick = !std::env::args().any(|argument| argument == "--bench");
    let rounds = if quick { 1 } else { rounds.max(1) };
    let entries = boxes();
    let mut index = SpatialIndex::default();
    index.rebuild(&entries);
    let inputs = Inputs {
        entries,
        index,
        // A tenth of the site each way: a hundredth of its area.
        views: windows(VIEW_QUERIES, SITE_WIDTH / 10.0, SITE_HEIGHT / 10.0, 7),
        cursors: windows(CURSOR_QUERIES, 2.0, 2.0, 11),
    };
    let mut cells = [
        "query_view",
        "query_cursor",
        "rebuild",
        "move_boxes",
        "query_view_again",
    ];
    let mut samples: Vec<(&str, Vec<f64>, usize)> =
        cells.iter().map(|cell| (*cell, Vec::new(), 0)).collect();
    for round in 0..rounds {
        if round % 2 == 1 {
            cells.reverse();
        }
        for cell in cells {
            for _ in 0..REPEATS {
                let (taken, found) = sample(cell, &inputs);
                if let Some((_, kept, ids)) = samples.iter_mut().find(|(name, _, _)| *name == cell)
                {
                    kept.push(taken);
                    *ids = found;
                }
            }
        }
        if round % 2 == 1 {
            cells.reverse();
        }
    }
    println!(
        "boxes={BOXES} cell_size={:.3} buckets={} oversized={}",
        inputs.index.cell_size(),
        inputs.index.bucket_count(),
        inputs.index.oversized_count()
    );
    for (cell, mut kept, ids) in samples {
        kept.sort_by(f64::total_cmp);
        let median = kept[kept.len() / 2];
        println!(
            "cell={cell} min_ms={:.3} median_ms={:.3} samples={} ids={ids}",
            kept[0],
            median,
            kept.len()
        );
    }
}
