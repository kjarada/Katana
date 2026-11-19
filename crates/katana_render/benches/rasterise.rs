//! The software rasteriser and the task pool under it, measured (CLAUDE.md section 5; the practice
//! of docs/testing.md, "Benchmarks"): what the 3D, elevation and section views pay a frame, and
//! what one dispatch of the pool costs. Written to measure the change that made the pool and the
//! rasteriser's disjoint writes safe Rust (docs/rust.md, "Unsafe that calls no C library,
//! removed"): the base build and the changed one are run alternately, each through this program.
//!
//! The scene is generated here: a terrain of 240 x 240 cells (115 200 triangles) with a sine
//! relief, 4 000 lines draped over it and 2 000 points, seen from the south-west at 1600 x 1000.
//! Cells, alternated round by round with the order reversed every round, each round taking
//! `REPEATS` samples of each:
//!
//! * `frame_pool` and `frame_pool_again`: the frame on the shared pool, under two names, the A/A
//!   control;
//! * `frame_one_thread`: the frame on a pool of no workers (the caller alone);
//! * `dispatch`: 1 000 `parallel_for` calls over 64 items doing nearly nothing, on the shared
//!   pool - the cost of a dispatch itself.
//!
//! Prints one `cell=... min_ms=... median_ms=... samples=...` line a cell. Run in Release:
//! `cargo bench --locked -p katana_render --bench rasterise` (or through `cargo test --release`,
//! which runs it too, once round); `KATANA_BENCH_ROUNDS` sets the rounds (default 7).

use std::hint::black_box;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use katana_core::task_pool::TaskPool;
use katana_math::Vec3;
use katana_render::{
    Camera, DrawList, Framebuffer, Projection, Rasterizer, RenderOptions, StandardView, rgba,
};

const WIDTH: i32 = 1600;
const HEIGHT: i32 = 1000;
const CELLS: usize = 240;
const SPACING: f64 = 4.0;
const LINES: usize = 4_000;
const POINTS: usize = 2_000;
const DISPATCHES: usize = 1_000;
const REPEATS: usize = 3;

fn scene() -> DrawList {
    let mut list = DrawList::default();
    let height = |x: f64, y: f64| 6.0 * (x / 37.0).sin() * (y / 53.0).cos() + 0.02 * x;
    let mut grid = Vec::with_capacity((CELLS + 1) * (CELLS + 1));
    for row in 0..=CELLS {
        for column in 0..=CELLS {
            let (x, y) = (column as f64 * SPACING, row as f64 * SPACING);
            let z = height(x, y);
            let shade = (128.0 + 12.0 * z).clamp(0.0, 255.0) as u8;
            grid.push(list.add_vertex(Vec3::new(x, y, z), rgba(shade / 2, shade, shade / 3)));
        }
    }
    let at = |row: usize, column: usize| grid[row * (CELLS + 1) + column];
    for row in 0..CELLS {
        for column in 0..CELLS {
            list.add_triangle(at(row, column), at(row, column + 1), at(row + 1, column + 1));
            list.add_triangle(at(row, column), at(row + 1, column + 1), at(row + 1, column));
        }
    }
    let span = CELLS as f64 * SPACING;
    for line in 0..LINES {
        let t = line as f64;
        let (x, y) = ((t * 7.3) % span, (t * 11.9) % span);
        let (x2, y2) = ((x + 25.0).min(span), (y + 9.0).min(span));
        let a = list.add_vertex(Vec3::new(x, y, height(x, y) + 0.1), rgba(255, 220, 0));
        let b = list.add_vertex(Vec3::new(x2, y2, height(x2, y2) + 0.1), rgba(255, 220, 0));
        list.add_line(a, b, 1.5, 0.0005);
    }
    for point in 0..POINTS {
        let t = point as f64;
        let (x, y) = ((t * 13.7) % span, (t * 5.3) % span);
        let p = list.add_vertex(Vec3::new(x, y, height(x, y) + 0.2), rgba(255, 60, 60));
        list.add_point(p, 4.0, 0.001);
    }
    list
}

fn camera() -> Camera {
    let mut camera = Camera::default();
    camera.set_viewport_size(WIDTH, HEIGHT);
    camera.set_projection(Projection::Perspective);
    camera.set_standard_view(StandardView::IsoSouthWest);
    let middle = CELLS as f64 * SPACING / 2.0;
    camera.set_target(Vec3::new(middle, middle, 0.0));
    camera.set_distance(1400.0);
    camera.set_depth_range(1.0, 5000.0);
    camera
}

/// One sample of `cell`, in milliseconds.
fn sample(cell: &str, list: &DrawList, camera: &Camera, one_thread: &TaskPool) -> f64 {
    let started;
    match cell {
        "dispatch" => {
            let pool = TaskPool::shared();
            let total = AtomicUsize::new(0);
            started = Instant::now();
            for _ in 0..DISPATCHES {
                pool.parallel_for(0, 64, |index| {
                    total.fetch_add(black_box(index), Ordering::Relaxed);
                });
            }
            black_box(total.load(Ordering::Relaxed));
        }
        _ => {
            let pool = if cell == "frame_one_thread" { one_thread } else { TaskPool::shared() };
            let mut target = Framebuffer::create(WIDTH, HEIGHT).expect("a 1600 x 1000 frame");
            let mut rasterizer = Rasterizer::default();
            let options = RenderOptions {
                background: rgba(30, 30, 30),
                pool: Some(pool),
                ..RenderOptions::default()
            };
            started = Instant::now();
            let stats = rasterizer
                .render(list, camera, &mut target, &options)
                .expect("the frame renders");
            black_box(stats);
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
    let list = scene();
    let camera = camera();
    let one_thread = TaskPool::new(0);
    let mut cells = ["frame_pool", "frame_one_thread", "dispatch", "frame_pool_again"];
    let mut samples: Vec<(&str, Vec<f64>)> = cells.iter().map(|cell| (*cell, Vec::new())).collect();
    for round in 0..rounds {
        if round % 2 == 1 {
            cells.reverse();
        }
        for cell in cells {
            for _ in 0..REPEATS {
                let taken = sample(cell, &list, &camera, &one_thread);
                if let Some((_, kept)) = samples.iter_mut().find(|(name, _)| *name == cell) {
                    kept.push(taken);
                }
            }
        }
        if round % 2 == 1 {
            cells.reverse();
        }
    }
    println!(
        "threads={} triangles={} lines={LINES} points={POINTS} size={WIDTH}x{HEIGHT}",
        TaskPool::shared().concurrency(),
        CELLS * CELLS * 2
    );
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
