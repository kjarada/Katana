use crate::support::simd::*;
use katana_core::cpu_features::SimdLevel;
use katana_core::task_pool::TaskPool;
use katana_geometry::Point2;
use katana_geometry::point_splat::*;
#[derive(Debug, PartialEq)]
struct Pixels {
    x: Vec<i32>,
    y: Vec<i32>,
}
fn project(projection: &PixelProjection, xs: &[f32], ys: &[f32]) -> Pixels {
    let mut output = Pixels {
        x: vec![0; xs.len()],
        y: vec![0; xs.len()],
    };
    project_to_pixels(projection, xs, ys, &mut output.x, &mut output.y);
    output
}
struct Source {
    x: f64,
    y: f64,
    colour: u32,
}
fn cloud_of(points: &[Source]) -> SplatCloud {
    let colours: Vec<_> = points.iter().map(|point| point.colour).collect();
    let mut bytes = Vec::with_capacity(points.len() * 24);
    for source in points {
        bytes.extend_from_slice(&source.x.to_ne_bytes());
        bytes.extend_from_slice(&source.y.to_ne_bytes());
        bytes.extend_from_slice(&source.colour.to_ne_bytes());
        bytes.extend_from_slice(&0_u32.to_ne_bytes());
    }
    build_splat_cloud(
        &bytes,
        bytes.get(8..).unwrap_or(&[]),
        24,
        points.len(),
        &colours,
        TaskPool::shared(),
    )
}
fn random_cloud(count: usize, width: f64, height: f64) -> Vec<Source> {
    let mut state = 12345_u64;
    let mut next = || {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (state >> 11) as f64 * (1.0 / 9007199254740992.0)
    };
    (0..count)
        .map(|index| Source {
            x: 500000.0 + width * next(),
            y: 7000000.0 + height * next(),
            colour: 0xff000000 | (((index as u64).wrapping_mul(2654435761) >> 8) as u32),
        })
        .collect()
}
fn splat_into(cloud: &SplatCloud, view: &SplatView, pool: &TaskPool) -> (Vec<u32>, SplatStats) {
    let mut image = vec![0; (view.width * view.height) as usize];
    let stats = splat_cloud(cloud, view, &mut image, view.width as usize, pool);
    (image, stats)
}
mod project_to_pixels {
    use super::*;
    #[test]
    fn a_points_pixel_is_its_projection_truncated_towards_zero_and_off_the_scale_is_no_pixel() {
        let projection = PixelProjection {
            half_width: 50.0,
            half_height: 40.0,
            centre_x: 10.0,
            centre_y: 20.0,
            scale: 2.0,
        };
        let xs = [
            12.25,
            9.75,
            -15.25,
            -20.0,
            500010.0,
            f32::NAN,
            -500015.0,
            12.25,
        ];
        let ys = [19.5, 20.25, 20.0, 20.0, 20.0, 20.0, 20.0, 19.5];
        let expected = Pixels {
            x: vec![54, 49, 0, -10, OFF_IMAGE, OFF_IMAGE, OFF_IMAGE, 54],
            y: vec![41, 39, 40, 40, OFF_IMAGE, OFF_IMAGE, OFF_IMAGE, 41],
        };
        assert_eq!(
            at_simd_level(SimdLevel::Scalar, || project(&projection, &xs, &ys)),
            expected
        );
        if kernels_available() {
            assert_eq!(
                at_simd_level(kernel_level(), || project(&projection, &xs, &ys)),
                expected
            );
        }
    }
    #[test]
    fn every_length_gives_the_scalar_loops_pixels_at_the_kernel_level() {
        let kernel = require_kernel();
        let awkward: [f32; 16] = [
            0.0,
            -0.0,
            0.5,
            -0.5,
            1.0,
            -1.0,
            3.75,
            -3.75,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::NAN,
            499995.0,
            -499995.0,
            499995.5,
            1.0e30,
            -1.0e-30,
        ];
        let projection = PixelProjection {
            half_width: 100.0,
            half_height: 80.0,
            centre_x: 0.25,
            centre_y: -0.5,
            scale: 2.0,
        };
        for length in 0..=40 {
            let xs: Vec<_> = (0..length)
                .map(|index| awkward[(index * 5 + length) % awkward.len()])
                .collect();
            let ys: Vec<_> = (0..length)
                .map(|index| awkward[(index * 3 + 7) % awkward.len()])
                .collect();
            let scalar = at_simd_level(SimdLevel::Scalar, || project(&projection, &xs, &ys));
            let kernel = at_simd_level(kernel, || project(&projection, &xs, &ys));
            assert_eq!(scalar, kernel);
        }
    }
}
mod splat_cloud {
    use super::*;
    #[test]
    fn the_image_is_the_same_at_every_thread_count_and_simd_level() {
        let cloud = cloud_of(&random_cloud(200000, 300.0, 200.0));
        for radius in [0, 1] {
            for budget in [50000, 1000000] {
                let view = SplatView {
                    centre: Point2::new(500150.0, 7000100.0),
                    scale: 2.3,
                    width: 640,
                    height: 480,
                    radius,
                    point_budget: budget,
                };
                let one = TaskPool::new(0);
                let reference =
                    at_simd_level(SimdLevel::Scalar, || splat_into(&cloud, &view, &one).0);
                for workers in [1, 2, 15] {
                    let pool = TaskPool::new(workers);
                    assert_eq!(
                        at_simd_level(SimdLevel::Scalar, || splat_into(&cloud, &view, &pool).0),
                        reference
                    );
                    if kernels_available() {
                        assert_eq!(
                            at_simd_level(kernel_level(), || splat_into(&cloud, &view, &pool).0),
                            reference
                        );
                    }
                }
            }
        }
    }
    #[test]
    fn where_points_share_a_pixel_the_last_in_the_files_order_is_on_top_and_the_budget_draws_the_first()
     {
        let cloud = cloud_of(&[
            Source {
                x: 10.0,
                y: 10.0,
                colour: 0xffff0000,
            },
            Source {
                x: 10.0,
                y: 10.0,
                colour: 0xff00ff00,
            },
            Source {
                x: 10.0,
                y: 10.0,
                colour: 0xff0000ff,
            },
        ]);
        let mut view = SplatView {
            centre: Point2::new(10.0, 10.0),
            width: 8,
            height: 8,
            ..Default::default()
        };
        let pool = TaskPool::new(0);
        assert_eq!(splat_into(&cloud, &view, &pool).0[4 * 8 + 4], 0xff0000ff);
        view.point_budget = 1;
        let (image, stats) = splat_into(&cloud, &view, &pool);
        assert_eq!(image[4 * 8 + 4], 0xffff0000);
        assert_eq!(stats.points_in_view, 3);
        assert_eq!(stats.points_drawn, 1);
    }
    #[test]
    fn over_budget_every_part_of_the_cloud_still_gets_its_share_of_points() {
        let cloud = cloud_of(&random_cloud(100000, 400.0, 400.0));
        let mut view = SplatView {
            centre: Point2::new(500200.0, 7000200.0),
            width: 400,
            height: 400,
            point_budget: 10000,
            ..Default::default()
        };
        let pool = TaskPool::new(3);
        let (image, stats) = splat_into(&cloud, &view, &pool);
        assert_eq!(stats.points_in_view, 100000);
        assert!(stats.points_drawn >= 10000);
        assert!(stats.points_drawn <= 10000 + cloud.tiles.len());
        for band_y in 0..4 {
            for band_x in 0..4 {
                let mut lit = 0;
                for row in band_y * 100..(band_y + 1) * 100 {
                    for column in band_x * 100..(band_x + 1) * 100 {
                        if image[row * 400 + column] != 0 {
                            lit += 1;
                        }
                    }
                }
                assert!(lit > 400);
            }
        }
        view.point_budget = 100000;
        let (_, stats) = splat_into(&cloud, &view, &pool);
        assert_eq!(stats.points_drawn, 100000);
    }
    #[test]
    fn points_that_cannot_be_drawn_are_left_out_of_the_display_copy() {
        let cloud = cloud_of(&[
            Source {
                x: 1.0,
                y: 2.0,
                colour: 1,
            },
            Source {
                x: f64::NAN,
                y: 2.0,
                colour: 2,
            },
            Source {
                x: 3.0,
                y: f64::INFINITY,
                colour: 3,
            },
            Source {
                x: 5.0,
                y: 6.0,
                colour: 4,
            },
        ]);
        assert_eq!(cloud.source_count, 4);
        assert_eq!(cloud.size(), 2);
        assert_eq!(cloud.origin.x, 3.0);
        assert_eq!(cloud.origin.y, 4.0);
        assert_eq!(cloud.colours[0] + cloud.colours[1], 5);
    }
    #[test]
    fn only_the_rows_the_cloud_can_reach_are_cleared_and_drawn() {
        let cloud = cloud_of(&[
            Source {
                x: 0.0,
                y: 40.0,
                colour: 0xff111111,
            },
            Source {
                x: 0.0,
                y: 30.0,
                colour: 0xff222222,
            },
        ]);
        let view = SplatView {
            width: 100,
            height: 100,
            ..Default::default()
        };
        let mut image = vec![0xdeadbeef; 100 * 100];
        let pool = TaskPool::new(0);
        let stats = splat_cloud(&cloud, &view, &mut image, 100, &pool);
        assert_eq!(stats.row_begin, 10);
        assert_eq!(stats.row_end, 21);
        assert_eq!(image[9 * 100 + 50], 0xdeadbeef);
        assert_eq!(image[21 * 100 + 50], 0xdeadbeef);
        assert_eq!(image[10 * 100 + 50], 0xff111111);
        assert_eq!(image[20 * 100 + 50], 0xff222222);
        assert_eq!(image[15 * 100 + 50], 0);
    }
}
