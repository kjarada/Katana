//! The twin of tests/qt_widgets/test_plan_cloud.cpp: the plan view's point clouds through
//! paint_plan - what drawing from the float display copy changes against the all-double splat it
//! replaced, that a frame is the same at both SIMD levels, that the kept layer image carries
//! nothing from one frame into the next, and what the point budget draws.

use std::sync::{Mutex, PoisonError};

use katana_core::cpu_features::{
    SimdLevel, active_simd_level, detected_simd_level, set_simd_level,
};
use katana_geometry::Box2;
use katana_interop::reference_data::{PointCloudLayer, PointColorMode, ReferenceData};
use katana_pointcloud::point_cloud_engine::PointCloudPoint;
use katana_window::canvas::Image;
use katana_window::plan_painter::{
    PlanFrame, PlanPaintCache, PlanPaintOptions, PlanPaintStats, PlanSource, paint_plan,
};

use crate::raster::paint_on;

const WIDTH: i32 = 800;
const HEIGHT: i32 = 500;

/// 300,000 points over 1500 m by 1000 m at map-grid coordinates, in random order, as a LAS file's
/// returns are.
fn cloud_of(mode: PointColorMode) -> PointCloudLayer {
    let mut cloud = PointCloudLayer {
        color_mode: mode,
        ..PointCloudLayer::default()
    };
    let mut state: u64 = 7;
    let mut next = || {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        // 0x1.0p-53
        (state >> 11) as f64 * (1.0 / 9_007_199_254_740_992.0)
    };
    cloud.points = (0..300_000)
        .map(|_| {
            let x = 300000.0 + 1500.0 * next();
            let y = 6250000.0 + 1000.0 * next();
            let z = 40.0 + 20.0 * next();
            PointCloudPoint {
                x,
                y,
                z,
                ..PointCloudPoint::default()
            }
        })
        .collect();
    for p in &cloud.points {
        cloud.bounds.min_x = cloud.bounds.min_x.min(p.x);
        cloud.bounds.min_y = cloud.bounds.min_y.min(p.y);
        cloud.bounds.min_z = cloud.bounds.min_z.min(p.z);
        cloud.bounds.max_x = cloud.bounds.max_x.max(p.x);
        cloud.bounds.max_y = cloud.bounds.max_y.max(p.y);
        cloud.bounds.max_z = cloud.bounds.max_z.max(p.z);
    }
    cloud.source_point_count = cloud.points.len() as u64;
    cloud
}

fn frame_around(extent: &Box2) -> PlanFrame<'static> {
    let mut frame = PlanFrame::default();
    frame.transform.resize(f64::from(WIDTH), f64::from(HEIGHT));
    frame.transform.fit(extent, 0.02);
    frame
}

fn paint(
    reference: &ReferenceData,
    frame: &PlanFrame,
    options: &PlanPaintOptions,
    cache: &mut PlanPaintCache,
) -> (Image, PlanPaintStats) {
    let mut image = Image::new(WIDTH as u32, HEIGHT as u32); // transparent
    let source = PlanSource {
        reference: Some(reference),
        ..PlanSource::default()
    };
    let mut stats = PlanPaintStats::default();
    paint_on(&mut image, |canvas| {
        stats = paint_plan(canvas, &source, frame, options, cache);
    });
    (image, stats)
}

/// The splat as it was before the display copy, for points of one size: double coordinates
/// straight from the stored points, in the file's order.
fn double_splat(cloud: &PointCloudLayer, frame: &PlanFrame) -> Vec<bool> {
    let mut lit = vec![false; (WIDTH * HEIGHT) as usize];
    let s = frame.transform.scale;
    for point in &cloud.points {
        let sx = 0.5 * f64::from(WIDTH) + (point.x - frame.transform.center.x) * s;
        let sy = 0.5 * f64::from(HEIGHT) - (point.y - frame.transform.center.y) * s;
        let on_grid = sx > -1.0e6 && sx < 1.0e6 && sy > -1.0e6 && sy < 1.0e6;
        if !on_grid {
            continue;
        }
        let px = sx as i32;
        let py = sy as i32;
        if px >= 0 && py >= 0 && px < WIDTH && py < HEIGHT {
            lit[(py * WIDTH + px) as usize] = true;
        }
    }
    lit
}

fn lit_near(lit: &[bool], x: i32, y: i32) -> bool {
    for dy in -1..=1 {
        for dx in -1..=1 {
            let (nx, ny) = (x + dx, y + dy);
            if nx >= 0 && ny >= 0 && nx < WIDTH && ny < HEIGHT && lit[(ny * WIDTH + nx) as usize] {
                return true;
            }
        }
    }
    false
}

/// The SIMD level is the process's: a switch holds one lock for its scope, so two tests never
/// switch it under each other (Rust runs a binary's cases on parallel threads of one process).
static LEVEL: Mutex<()> = Mutex::new(());

/// Puts the process at a level for a scope and back afterwards (the C++'s LevelScope).
struct LevelScope {
    previous: SimdLevel,
    ok: bool,
    _lock: std::sync::MutexGuard<'static, ()>,
}

impl LevelScope {
    fn new(level: SimdLevel) -> Self {
        let lock = LEVEL.lock().unwrap_or_else(PoisonError::into_inner);
        let previous = active_simd_level();
        let ok = set_simd_level(level).is_ok();
        Self {
            previous,
            ok,
            _lock: lock,
        }
    }
}

impl Drop for LevelScope {
    fn drop(&mut self) {
        let _ = set_simd_level(self.previous);
    }
}

mod plan_painter_cloud {
    use super::*;

    #[test]
    fn float_offsets_move_a_few_points_on_pixel_edges_by_one_pixel_and_none_further() {
        // One colour, so which point is on top cannot show: only where each point lands. At this
        // view a pixel is about 1.9 m; the stored offsets are within 750 m of the origin, where a
        // float's spacing is at most 2^-14 m, so storing one moves it at most 0.03 mm, 1.6e-5 of
        // a pixel. A point changes pixel only when it lies that close to a pixel edge, and then by
        // one pixel.
        let mut reference = ReferenceData::new();
        reference.add(cloud_of(PointColorMode::Flat));
        let cloud = &reference.point_clouds()[0];
        let frame = frame_around(&cloud.world_bounds());
        let options = PlanPaintOptions {
            cloud_point_budget: usize::MAX,
            ..PlanPaintOptions::default()
        };
        let mut cache = PlanPaintCache::default();
        let (image, _) = paint(&reference, &frame, &options, &mut cache);
        let mut drawn = vec![false; (WIDTH * HEIGHT) as usize];
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                drawn[(y * WIDTH + x) as usize] = image.pixel(x as u32, y as u32).a != 0;
            }
        }
        let expected = double_splat(cloud, &frame);
        let mut differing = 0;
        let mut lit = 0;
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                let at = (y * WIDTH + x) as usize;
                lit += i32::from(expected[at]);
                if drawn[at] != expected[at] {
                    differing += 1;
                    assert!(
                        if drawn[at] {
                            lit_near(&expected, x, y)
                        } else {
                            lit_near(&drawn, x, y)
                        },
                        "pixel {x}, {y} moved more than one pixel"
                    );
                }
            }
        }
        assert!(lit > 100_000, "{lit}");
        // Measured in the C++: 3 of 200,639 lit pixels. One in ten thousand (20 here) is room for
        // that, and still fails if the offsets lose precision - stored from a far origin, or
        // projected in float.
        assert!(
            differing <= lit / 10_000,
            "{differing} of {lit} lit pixels differ"
        );
    }

    #[test]
    fn a_frame_is_the_same_at_both_simd_levels() {
        // The kernel level is AVX2 on x86-64 or NEON on 64-bit ARM.
        let kernel = detected_simd_level();
        if kernel == SimdLevel::Scalar {
            eprintln!("no SIMD kernels run on this processor: only the scalar path can run here");
            return;
        }
        let mut reference = ReferenceData::new();
        reference.add(cloud_of(PointColorMode::Elevation));
        let mut frame = frame_around(&reference.point_clouds()[0].world_bounds());
        frame.transform.scale *= 1.7;
        let options = PlanPaintOptions {
            cloud_point_budget: 120_000,
            ..PlanPaintOptions::default()
        };
        let mut images = Vec::new();
        for level in [SimdLevel::Scalar, kernel] {
            let scope = LevelScope::new(level);
            assert!(scope.ok);
            let mut cache = PlanPaintCache::default();
            images.push(paint(&reference, &frame, &options, &mut cache).0);
        }
        assert!(images[0] == images[1]);
    }

    #[test]
    fn the_kept_layer_image_carries_nothing_from_one_frame_into_the_next() {
        // A frame after a zoom and a pan, drawn with the cache the frame before used, is the frame
        // a fresh cache draws.
        let mut reference = ReferenceData::new();
        reference.add(cloud_of(PointColorMode::Elevation));
        let mut frame = frame_around(&reference.point_clouds()[0].world_bounds());
        let options = PlanPaintOptions::default();
        let mut kept = PlanPaintCache::default();
        let _ = paint(&reference, &frame, &options, &mut kept);
        frame.transform.scale *= 3.0;
        frame.transform.pan_by_pixels(37.0, -21.0);
        let (second, _) = paint(&reference, &frame, &options, &mut kept);
        let mut fresh = PlanPaintCache::default();
        assert!(second == paint(&reference, &frame, &options, &mut fresh).0);
    }

    #[test]
    fn over_the_point_budget_a_frame_draws_the_budgets_share_of_the_points_in_view() {
        let mut reference = ReferenceData::new();
        reference.add(cloud_of(PointColorMode::Elevation));
        let frame = frame_around(&reference.point_clouds()[0].world_bounds());
        let mut options = PlanPaintOptions {
            cloud_point_budget: 30_000,
            ..PlanPaintOptions::default()
        };
        let mut cache = PlanPaintCache::default();
        let (_, stats) = paint(&reference, &frame, &options, &mut cache);
        assert_eq!(stats.cloud_points_in_view, 300_000);
        // Each of the ~290 tiles (300,000 / 1024) rounds its share up.
        assert!(stats.cloud_points_drawn >= 30_000);
        assert!(stats.cloud_points_drawn <= 30_000 + 300);
        options.cloud_point_budget = 300_000;
        let (_, stats) = paint(&reference, &frame, &options, &mut cache);
        assert_eq!(stats.cloud_points_drawn, 300_000);
    }
}
