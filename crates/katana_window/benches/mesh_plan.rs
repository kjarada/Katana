//! The meshes in plan, measured (docs/plan_view.md, "Meshes in plan"; CLAUDE.md section 5): a
//! plan view's paint of a synthetic design model the size of the one that showed the need - 640
//! meshes of 1 600 faces, 1 024 000 in all, laid as kerb and pavement strips over a 2 km by 1 km
//! site with crossfalls and kerb steps - drawn as footprints (the plan before 2026-11-19) and as
//! faces, through paint_plan onto the raster canvas at 1600 x 1000, as the window paints.
//!
//! Cells, alternated round by round with the order reversed every round (docs/testing.md,
//! "Benchmarks"), each round taking `REPEATS` samples of each:
//!
//! * `footprint` and `footprint_again`: the same paint under two names, the A/A control;
//! * `faces_made`: the faces at the extents, the image made afresh (the view moved a third of a
//!   pixel, onto another pixel grid);
//! * `faces_kept`: the same view again, the kept image laid down (a selection, a command);
//! * `faces_zoomed`: eight times closer, the image made afresh;
//! * `image_one_thread` and `image_pool`: the faces' image alone, on one thread and on the shared
//!   pool.
//!
//! Prints one `cell=... min_ms=... median_ms=... samples=...` line a cell, and the time to work
//! out the meshes once (their boxes, shades and rims) and what the faces covered. Never the
//! owner's data: the model is generated here. Run in Release:
//! `cargo bench --locked -p katana_window --bench mesh_plan` (or through `cargo test --release`,
//! which runs it too); `KATANA_BENCH_ROUNDS` sets the rounds (default 7).

use std::time::Instant;

use katana_cad::scene::{MeshPlanDisplay, SceneMesh};
use katana_cad::terrain::surface_plan::plan_image_grid;
use katana_core::task_pool::TaskPool;
use katana_geometry::{Box2, Point2, Point3, TriangleMesh};
use katana_render::rgba;
use katana_window::canvas::{Brush, Canvas, Color, Image, RasterCanvas, RectF};
use katana_window::mesh_plan_painter::{MeshInk, MeshPlanCache, mesh_faces_image};
use katana_window::plan_painter::{
    PlanFrame, PlanPaintCache, PlanPaintOptions, PlanPaintStats, PlanSource, paint_plan,
    visible_box,
};

const WIDTH: f64 = 1600.0;
const HEIGHT: f64 = 1000.0;
const REPEATS: usize = 3;

/// A strip `length` long and `width` wide from `origin` along `heading`, `along` x `across`
/// quads, falling 2 % across and stepping up 0.15 at its far edge as a kerb does, rising 1 in 200
/// along.
fn strip(origin: Point2, heading: f64, length: f64, width: f64, z: f64) -> TriangleMesh {
    let (along, across) = (40usize, 20usize);
    let (c, s) = (heading.cos(), heading.sin());
    let mut mesh = TriangleMesh::default();
    for i in 0..=along {
        for j in 0..=across {
            let u = length * i as f64 / along as f64;
            let v = width * j as f64 / across as f64;
            let kerb = if j == across { 0.15 } else { 0.0 };
            mesh.vertices.push(Point3::new(
                origin.x + u * c - v * s,
                origin.y + u * s + v * c,
                z + u / 200.0 - 0.02 * v + kerb,
            ));
        }
    }
    let row = (across + 1) as u32;
    for i in 0..along as u32 {
        for j in 0..across as u32 {
            let a = i * row + j;
            let b = a + row;
            mesh.faces.push([a, b, b + 1]);
            mesh.faces.push([a, b + 1, a + 1]);
        }
    }
    mesh
}

fn model() -> Vec<TriangleMesh> {
    let mut state: u64 = 2026;
    let mut next = || {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (state >> 11) as f64 * (1.0 / 9_007_199_254_740_992.0)
    };
    (0..640)
        .map(|_| {
            strip(
                Point2::new(300_000.0 + 2000.0 * next(), 6_250_000.0 + 1000.0 * next()),
                std::f64::consts::TAU * next(),
                60.0 + 80.0 * next(),
                2.0 + 8.0 * next(),
                20.0 + 10.0 * next(),
            )
        })
        .collect()
}

fn frame_of(bounds: &Box2, zoom: f64, nudge: f64) -> PlanFrame<'static> {
    let mut frame = PlanFrame::default();
    frame.transform.resize(WIDTH, HEIGHT);
    frame.transform.fit(bounds, 0.02);
    frame.transform.scale *= zoom;
    frame.transform.center.x += nudge / frame.transform.scale;
    frame
}

fn paint(
    source: &PlanSource<'_>,
    frame: &PlanFrame<'_>,
    cache: &mut PlanPaintCache,
    image: &mut Image,
) -> f64 {
    let options = PlanPaintOptions::default();
    let started = Instant::now();
    RasterCanvas::paint_image(image, &mut |canvas: &mut dyn Canvas| {
        canvas.fill_rect(
            RectF::new(0.0, 0.0, WIDTH, HEIGHT),
            &Brush::solid(Color::rgb(0x1e, 0x23, 0x29)),
        );
        let _ = paint_plan(canvas, source, frame, &options, cache);
    });
    started.elapsed().as_secs_f64() * 1.0e3
}

fn summary(name: &str, samples: &mut [f64]) {
    samples.sort_by(f64::total_cmp);
    let median = samples[samples.len() / 2];
    println!(
        "cell={name} min_ms={:.2} median_ms={:.2} samples={}",
        samples[0],
        median,
        samples.len()
    );
}

fn main() {
    let rounds = std::env::var("KATANA_BENCH_ROUNDS")
        .ok()
        .and_then(|text| text.parse::<usize>().ok())
        .unwrap_or(7)
        .max(1);
    let meshes = model();
    let triangles: usize = meshes.iter().map(TriangleMesh::triangle_count).sum();
    let items: Vec<SceneMesh<'_>> = meshes
        .iter()
        .map(|mesh| SceneMesh {
            mesh: Some(mesh),
            flat_color: rgba(190, 170, 140),
            ..SceneMesh::default()
        })
        .collect();
    let mut bounds = Box2::default();
    for mesh in &meshes {
        for vertex in &mesh.vertices {
            bounds.expand(Point2::new(vertex.x, vertex.y));
        }
    }
    println!(
        "meshes={} triangles={triangles} view={WIDTH}x{HEIGHT} threads={}",
        meshes.len(),
        TaskPool::shared().concurrency()
    );
    let source_with = |display: MeshPlanDisplay| PlanSource {
        meshes: Some(&items),
        mesh_display: display,
        ..PlanSource::default()
    };
    let footprint = source_with(MeshPlanDisplay::Footprint);
    let faces = source_with(MeshPlanDisplay::Faces);
    let mut image = Image::new(WIDTH as u32, HEIGHT as u32);

    // The meshes worked out once: the first paint of the faces pays for it.
    let mut cache = PlanPaintCache::default();
    let first = paint(&faces, &frame_of(&bounds, 1.0, 0.0), &mut cache, &mut image);
    let mut stats_cache = PlanPaintCache::default();
    let mut stats = PlanPaintStats::default();
    RasterCanvas::paint_image(&mut image, &mut |canvas: &mut dyn Canvas| {
        stats = paint_plan(
            canvas,
            &faces,
            &frame_of(&bounds, 1.0, 0.0),
            &PlanPaintOptions::default(),
            &mut stats_cache,
        );
    });
    println!(
        "first_faces_paint_ms={first:.2} meshes_drawn={} triangles_in_view={} pixels_covered={}",
        stats.meshes_drawn, stats.mesh_triangles_in_view, stats.mesh_pixels_covered
    );

    let grid = plan_image_grid(
        &visible_box(&frame_of(&bounds, 1.0, 0.0)),
        &bounds,
        1.0 / frame_of(&bounds, 1.0, 0.0).transform.scale,
        16_000_000,
    );
    let shown: Vec<&SceneMesh<'_>> = items.iter().collect();
    let one = TaskPool::new(0);
    let mut image_cache = MeshPlanCache::new();
    let _ = mesh_faces_image(&shown, &grid, MeshInk::SCREEN, &mut image_cache, &one);

    let names = [
        "footprint",
        "faces_made",
        "faces_kept",
        "faces_zoomed",
        "image_one_thread",
        "image_pool",
        "footprint_again",
    ];
    let mut samples: Vec<Vec<f64>> = vec![Vec::new(); names.len()];
    let mut nudge = 0.0;
    for round in 0..rounds {
        let order: Vec<usize> = if round % 2 == 0 {
            (0..names.len()).collect()
        } else {
            (0..names.len()).rev().collect()
        };
        for &cell in &order {
            for _ in 0..REPEATS {
                // A third of a pixel: a pan by whole pixels with every mesh in view lays the kept
                // image down again, which is faces_kept's case, not this one's.
                nudge = if nudge == 0.0 { 1.0 / 3.0 } else { 0.0 };
                let taken = match names[cell] {
                    "footprint" | "footprint_again" => paint(
                        &footprint,
                        &frame_of(&bounds, 1.0, 0.0),
                        &mut cache,
                        &mut image,
                    ),
                    "faces_made" => paint(
                        &faces,
                        &frame_of(&bounds, 1.0, nudge),
                        &mut cache,
                        &mut image,
                    ),
                    "faces_kept" => {
                        // Made once at this view, then laid down: the sample is the second paint.
                        let at = frame_of(&bounds, 1.0, 0.5);
                        let _ = paint(&faces, &at, &mut cache, &mut image);
                        paint(&faces, &at, &mut cache, &mut image)
                    }
                    "faces_zoomed" => paint(
                        &faces,
                        &frame_of(&bounds, 8.0, nudge),
                        &mut cache,
                        &mut image,
                    ),
                    "image_one_thread" => {
                        let started = Instant::now();
                        let _ = mesh_faces_image(
                            &shown,
                            &grid,
                            MeshInk::SCREEN,
                            &mut image_cache,
                            &one,
                        );
                        started.elapsed().as_secs_f64() * 1.0e3
                    }
                    _ => {
                        let started = Instant::now();
                        let _ = mesh_faces_image(
                            &shown,
                            &grid,
                            MeshInk::SCREEN,
                            &mut image_cache,
                            TaskPool::shared(),
                        );
                        started.elapsed().as_secs_f64() * 1.0e3
                    }
                };
                samples[cell].push(taken);
            }
        }
    }
    for (name, cell) in names.iter().zip(&mut samples) {
        summary(name, cell);
    }
}
