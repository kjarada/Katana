//! The GPU renderer against the software rasteriser, pixel by pixel, within a
//! tolerance (docs/gpu.md, "Testing"): the twin of tests/gpu/test_gpu_render.cpp.
//!
//! Every device case runs twice: on the machine's GPU and on its software
//! device - lavapipe, Mesa's Vulkan on the CPU, or WARP, Windows' software
//! device - which is what a GPU-less runner or cloud session still tests. A
//! device that is missing is skipped, saying so.
//!
//! A GPU frame is never bit-identical to the CPU's: 4x multisampling blends
//! edge pixels, lines get an analytic antialiased fringe, and the two fill
//! rules differ at exact pixel centres. So the comparisons are:
//!
//! ```text
//! COVERAGE  a pixel is covered when it is at least half as bright as the
//!           primitive over it would make it; the images may disagree only
//!           along edges, and the tolerance is a count of edge pixels worked
//!           out from the shape's perimeter, not a percentage;
//! COLOUR    over pixels whose whole 3x3 neighbourhood is covered in both -
//!           where no edge reaches - the colours agree within 2 levels, the
//!           most two correctly rounded interpolations can differ by.
//! ```

use crate::gpu_support::{
    DEVICES, Image, brightness, compare, cpu_render, describe, device_label, gpu_or_skip, label,
    save_for_looking,
};
use crate::render_support::near_because;
use katana_gpu::{
    ALL_EXPANSIONS, FrameSettings, GpuDevice, GpuFrameStats, GpuSceneData, LightingMode,
    OffscreenGpu, PointCloudData, compile_wgsl, pack_draw_list_with_origin, pack_point_cloud,
    precompile_shaders,
};
use katana_math::Vec3;
use katana_render::{
    Camera, DrawList, Projection, Rgba, StandardView, VertexIndex, blue_of, green_of, red_of, rgba,
};
use std::f64::consts::PI;

const BLACK: Rgba = rgba(0, 0, 0);

// Looking straight down, orthographic, one world unit per pixel, the world
// origin at the image centre: pixel (px, py) is world (px - w/2, h/2 - py).
fn plan_camera(width: i32, height: i32) -> Camera {
    let mut camera = Camera::default();
    camera.set_viewport_size(width, height);
    camera.set_projection(Projection::Orthographic);
    camera.set_standard_view(StandardView::Top);
    camera.set_target(Vec3::new(0.0, 0.0, 0.0));
    camera.set_distance(1000.0);
    camera.set_orthographic_height(f64::from(height));
    camera.set_depth_range(1.0, 2000.0);
    camera
}

fn black_background() -> FrameSettings {
    FrameSettings {
        background: BLACK,
        ..FrameSettings::default()
    }
}

// The C++ fixture's render(): one frame read back.
fn render(gpu: &mut OffscreenGpu, camera: &Camera, settings: &FrameSettings) -> Image {
    let mut pixels = Vec::new();
    if let Err(error) = gpu.render_frame(camera, settings, Some(&mut pixels)) {
        panic!("{}", error.describe());
    }
    pixels
}

// One frame, not read back: its stats.
fn frame(gpu: &mut OffscreenGpu, camera: &Camera, settings: &FrameSettings) -> GpuFrameStats {
    match gpu.render_frame(camera, settings, None) {
        Ok(stats) => stats,
        Err(error) => panic!("{}", error.describe()),
    }
}

// A 40 m square of ground, 8 x 8 cells, rolling a few metres, coloured by
// height: a small TIN, with its outline as lines. `offset` moves it.
fn site(offset: Vec3) -> DrawList {
    let mut list = DrawList::default();
    const CELLS: i32 = 8;
    const SIZE: f64 = 40.0;
    let step = SIZE / f64::from(CELLS);
    for j in 0..=CELLS {
        for i in 0..=CELLS {
            let x = -SIZE * 0.5 + step * f64::from(i);
            let y = -SIZE * 0.5 + step * f64::from(j);
            let z = 3.0 * (x * 0.15).sin() * (y * 0.11).cos();
            let shade = (120.0 + 40.0 * z / 3.0) as u8;
            list.add_vertex(offset + Vec3::new(x, y, z), rgba(shade, 200, 255 - shade));
        }
    }
    let at = |i: i32, j: i32| (j * (CELLS + 1) + i) as VertexIndex;
    for j in 0..CELLS {
        for i in 0..CELLS {
            list.add_triangle(at(i, j), at(i + 1, j), at(i + 1, j + 1));
            list.add_triangle(at(i, j), at(i + 1, j + 1), at(i, j + 1));
        }
    }
    let white = rgba(255, 255, 255);
    list.add_segment(
        offset + Vec3::new(-20.0, -20.0, 6.0),
        offset + Vec3::new(20.0, -20.0, 6.0),
        white,
        1.0,
        0.0,
    );
    list.add_segment(
        offset + Vec3::new(20.0, -20.0, 6.0),
        offset + Vec3::new(20.0, 20.0, 6.0),
        white,
        2.0,
        0.0,
    );
    list
}

fn site_camera(offset: Vec3, width: i32, height: i32) -> Camera {
    let mut camera = Camera::default();
    camera.set_viewport_size(width, height);
    camera.set_projection(Projection::Perspective);
    camera.set_standard_view(StandardView::IsoSouthWest);
    camera.set_target(offset);
    camera.set_distance(75.0);
    camera.set_depth_range(0.5, 1000.0);
    camera
}

fn add_quad(list: &mut DrawList, corners: [Vec3; 4], color: Rgba) {
    let a = list.add_vertex(corners[0], color);
    let b = list.add_vertex(corners[1], color);
    let c = list.add_vertex(corners[2], color);
    let d = list.add_vertex(corners[3], color);
    list.add_triangle(a, b, c);
    list.add_triangle(a, c, d);
}

// A 40 x 25 m red wall facing south, standing on y = 0.
fn red_wall() -> DrawList {
    let mut list = DrawList::default();
    add_quad(
        &mut list,
        [
            Vec3::new(-20.0, 0.0, 0.0),
            Vec3::new(20.0, 0.0, 0.0),
            Vec3::new(20.0, 0.0, 25.0),
            Vec3::new(-20.0, 0.0, 25.0),
        ],
        rgba(255, 0, 0),
    );
    list
}

// The site's relief as a single-coloured TIN, and marks lying on it: size-5
// points at its vertices (the scene builder's default point size - survey
// points on the TIN built from them) or lines along its grid rows. Pure
// green under pure magenta, so a mark pixel is told from the surface by its
// red and blue alone.
const SLOPE_CELLS: i32 = 8;
const SLOPE_SIZE: f64 = 40.0;
const MARK_COLOUR: Rgba = rgba(255, 0, 255);

fn slope_vertex(i: i32, j: i32) -> Vec3 {
    let step = SLOPE_SIZE / f64::from(SLOPE_CELLS);
    let x = -SLOPE_SIZE * 0.5 + step * f64::from(i);
    let y = -SLOPE_SIZE * 0.5 + step * f64::from(j);
    Vec3::new(x, y, 3.0 * (x * 0.15).sin() * (y * 0.11).cos())
}

fn add_sloped_surface(list: &mut DrawList) {
    let first = list.positions.len() as VertexIndex;
    for j in 0..=SLOPE_CELLS {
        for i in 0..=SLOPE_CELLS {
            list.add_vertex(slope_vertex(i, j), rgba(0, 150, 0));
        }
    }
    let at = |i: i32, j: i32| first + (j * (SLOPE_CELLS + 1) + i) as VertexIndex;
    for j in 0..SLOPE_CELLS {
        for i in 0..SLOPE_CELLS {
            list.add_triangle(at(i, j), at(i + 1, j), at(i + 1, j + 1));
            list.add_triangle(at(i, j), at(i + 1, j + 1), at(i, j + 1));
        }
    }
}

fn slope_points(size: f32) -> DrawList {
    let mut list = DrawList::default();
    for j in 0..=SLOPE_CELLS {
        for i in 0..=SLOPE_CELLS {
            let vertex = list.add_vertex(slope_vertex(i, j), MARK_COLOUR);
            list.add_point(vertex, size, 0.0);
        }
    }
    list
}

// Every TIN edge along a grid row, in pieces a cell long, as a TIN's edges are
// drawn: each piece lies exactly in the surface.
fn slope_rows(width: f32) -> DrawList {
    let mut list = DrawList::default();
    for j in 0..=SLOPE_CELLS {
        for i in 0..SLOPE_CELLS {
            list.add_segment(
                slope_vertex(i, j),
                slope_vertex(i + 1, j),
                MARK_COLOUR,
                width,
                0.0,
            );
        }
    }
    list
}

// Over black a mark pixel's red is its coverage, 255 where the mark covers the
// whole pixel; over the green surface an uncut mark blends to the same red,
// since the surface has none. At least half covered counts as a mark pixel,
// and it shows over the surface when it keeps that red to within 16 levels -
// a sixteenth of it, where losing one of the four samples to the surface
// costs a quarter.
fn mark_pixel(alone: Rgba) -> bool {
    red_of(alone) >= 128
}

fn mark_shows(alone: Rgba, over: Rgba) -> bool {
    i32::from(red_of(over)) + 16 >= i32::from(red_of(alone))
}

#[derive(Clone, Copy, Debug, Default)]
struct Kept {
    marks: usize,
    shown: usize,
}

// The marks drawn alone, then over the sloped surface: how many mark pixels
// there are, and how many still show over the surface.
fn kept_over(
    gpu: &mut OffscreenGpu,
    device: GpuDevice,
    marks: &DrawList,
    pixel_ratio: f64,
    name: &str,
    view: &Camera,
) -> Kept {
    let settings = FrameSettings {
        pixel_ratio,
        ..black_background()
    };
    gpu.renderer().set_draw_list(marks);
    let alone = render(gpu, view, &settings);
    let mut on_surface = DrawList::default();
    add_sloped_surface(&mut on_surface);
    let base = on_surface.positions.len() as VertexIndex;
    on_surface.positions.extend_from_slice(&marks.positions);
    on_surface.colors.extend_from_slice(&marks.colors);
    for &line in &marks.lines {
        let mut line = line;
        line.a += base;
        line.b += base;
        on_surface.lines.push(line);
    }
    for &point in &marks.points {
        let mut point = point;
        point.a += base;
        on_surface.points.push(point);
    }
    gpu.renderer().set_draw_list(&on_surface);
    let over = render(gpu, view, &settings);
    save_for_looking(&label(name, device), &over, gpu.width(), gpu.height());
    let mut kept = Kept::default();
    for (&alone, &over) in alone.iter().zip(&over) {
        if mark_pixel(alone) {
            kept.marks += 1;
            kept.shown += usize::from(mark_shows(alone, over));
        }
    }
    kept
}

mod gpu_render {
    use super::*;

    #[test]
    fn a_shaded_triangle_covers_the_software_paths_pixels_in_its_colours() {
        const WIDTH: i32 = 96;
        const HEIGHT: i32 = 64;
        for device in DEVICES {
            let Some(mut gpu) = gpu_or_skip(WIDTH, HEIGHT, device) else {
                continue;
            };
            // Red, green and blue corners: every interpolated colour has r + g
            // + b = 255, so "covered" is simply brightness >= 128.
            let mut list = DrawList::default();
            let a = list.add_vertex(Vec3::new(-30.0, -20.0, 0.0), rgba(255, 0, 0));
            let b = list.add_vertex(Vec3::new(30.0, -20.0, 0.0), rgba(0, 255, 0));
            let c = list.add_vertex(Vec3::new(0.0, 25.0, 0.0), rgba(0, 0, 255));
            list.add_triangle(a, b, c);
            let camera = plan_camera(WIDTH, HEIGHT);

            gpu.renderer().set_draw_list(&list);
            let gpu_image = render(&mut gpu, &camera, &black_background());
            let cpu_image = cpu_render(&list, &camera, BLACK);
            save_for_looking(&label("triangle_gpu", device), &gpu_image, WIDTH, HEIGHT);
            save_for_looking("triangle_cpu", &cpu_image, WIDTH, HEIGHT);

            let result = compare(&cpu_image, &gpu_image, WIDTH, HEIGHT, BLACK, 128);
            let trace = format!("{}: {}", device_label(device), describe(&result));
            // Area: base 60 x height 45 / 2 = 1350 px.
            near_because(result.covered_a as f64, 1350.0, 60.0, &trace);
            // Perimeter: 60 + 2 * sqrt(30^2 + 45^2) = 60 + 2 * 54.08 = 168 px.
            // The two can only disagree on a pixel whose centre is within a
            // quarter pixel of an edge (closer to it than two of the four
            // samples), a band half a pixel wide: at most 168 / 2 = 84 pixels.
            assert!(result.coverage_mismatches <= 84, "{trace}");
            assert!(result.interior_pixels > 900, "{trace}");
            assert!(result.max_interior_difference <= 2, "{trace}");
        }
    }

    #[test]
    fn overlapping_surfaces_in_perspective_hide_each_other_as_the_software_path_does() {
        const WIDTH: i32 = 128;
        const HEIGHT: i32 = 96;
        for device in DEVICES {
            let Some(mut gpu) = gpu_or_skip(WIDTH, HEIGHT, device) else {
                continue;
            };
            // A red ground square and a green square 2 m above it, half
            // overlapping, and a blue wall standing in front of both.
            let mut list = DrawList::default();
            add_quad(
                &mut list,
                [
                    Vec3::new(-20.0, -20.0, 0.0),
                    Vec3::new(20.0, -20.0, 0.0),
                    Vec3::new(20.0, 20.0, 0.0),
                    Vec3::new(-20.0, 20.0, 0.0),
                ],
                rgba(255, 0, 0),
            );
            add_quad(
                &mut list,
                [
                    Vec3::new(0.0, -10.0, 2.0),
                    Vec3::new(30.0, -10.0, 2.0),
                    Vec3::new(30.0, 30.0, 2.0),
                    Vec3::new(0.0, 30.0, 2.0),
                ],
                rgba(0, 255, 0),
            );
            add_quad(
                &mut list,
                [
                    Vec3::new(-15.0, -25.0, 0.0),
                    Vec3::new(5.0, -25.0, 0.0),
                    Vec3::new(5.0, -25.0, 12.0),
                    Vec3::new(-15.0, -25.0, 12.0),
                ],
                rgba(0, 0, 255),
            );
            let mut camera = Camera::default();
            camera.set_viewport_size(WIDTH, HEIGHT);
            camera.set_projection(Projection::Perspective);
            camera.set_standard_view(StandardView::IsoSouthWest);
            assert!(camera.frame(list.bounds()));

            gpu.renderer().set_draw_list(&list);
            // The software path's depths: the camera's own near and far
            // (frame() fitted them to the box), reversed, nothing drawn past
            // the far plane.
            let settings = FrameSettings {
                infinite_far: false,
                ..black_background()
            };
            let gpu_image = render(&mut gpu, &camera, &settings);
            let cpu_image = cpu_render(&list, &camera, BLACK);
            save_for_looking(&label("overlap_gpu", device), &gpu_image, WIDTH, HEIGHT);
            save_for_looking("overlap_cpu", &cpu_image, WIDTH, HEIGHT);

            let result = compare(&cpu_image, &gpu_image, WIDTH, HEIGHT, BLACK, 128);
            let trace = format!("{}: {}", device_label(device), describe(&result));
            // Framed by the box's projected corners (Camera::frame,
            // docs/render.md "Framing"). The box is (-20, -25, 0)-(30, 30, 12),
            // centre (5, 2.5, 6); seen from the SW isometric direction the
            // vertical fit decides, and the eye stands 94.2 m from the centre.
            // The squares' corners then project, in pixels, to
            //   red    (61.1, 88.1) (96.9, 57.9) (62.2, 39.7) (26.7, 57.9)  1698 px
            //   green  (71.1, 62.0) (93.6, 45.9) (62.4, 31.4) (39.7, 41.7)   806 px
            //   wall   (72.7, 88.1) (91.5, 70.8) (94.1, 57.8) (73.7, 74.5)   231 px
            // (shoelace areas), and their union - everything drawn is lit
            // against black - is 2284 px.
            near_because(result.covered_a as f64, 2284.0, 250.0, &trace);
            // Disagreement only where an outline or the green square's edge
            // over the red crosses a pixel: the outlines above are 172, 124
            // and 79 px long, 375 px of edge, and at most half of those pixels
            // (the quarter-pixel band above) can flip either way.
            assert!(result.coverage_mismatches <= 188, "{trace}");
            assert!(result.differing_pixels <= 360, "{trace}");
            assert!(result.max_interior_difference <= 2, "{trace}");
        }
    }

    #[test]
    fn lines_and_points_cover_the_software_paths_pixels_within_their_antialiased_edges() {
        const WIDTH: i32 = 96;
        const HEIGHT: i32 = 64;
        for device in DEVICES {
            let Some(mut gpu) = gpu_or_skip(WIDTH, HEIGHT, device) else {
                continue;
            };
            // Pixel centres are at half-integers in the plan camera, so a line
            // along y = 0.5 runs exactly along pixel row 31's centres.
            let white = rgba(255, 255, 255);
            let mut list = DrawList::default();
            // 1 px, 81 long.
            list.add_segment(
                Vec3::new(-40.0, 0.5, 0.0),
                Vec3::new(40.0, 0.5, 0.0),
                white,
                1.0,
                0.0,
            );
            // 3 px, 51 long.
            list.add_segment(
                Vec3::new(-20.5, -25.0, 0.0),
                Vec3::new(-20.5, 25.0, 0.0),
                white,
                3.0,
                0.0,
            );
            // The diagonal.
            list.add_segment(
                Vec3::new(0.0, -25.0, 0.0),
                Vec3::new(40.0, 15.0, 0.0),
                white,
                1.0,
                0.0,
            );
            let point = list.add_vertex(Vec3::new(30.5, -20.5, 0.0), white);
            list.add_point(point, 5.0, 0.0);
            let camera = plan_camera(WIDTH, HEIGHT);

            gpu.renderer().set_draw_list(&list);
            let gpu_image = render(&mut gpu, &camera, &black_background());
            let cpu_image = cpu_render(&list, &camera, BLACK);
            save_for_looking(&label("lines_gpu", device), &gpu_image, WIDTH, HEIGHT);
            save_for_looking("lines_cpu", &cpu_image, WIDTH, HEIGHT);

            // Half of white's 765.
            let result = compare(&cpu_image, &gpu_image, WIDTH, HEIGHT, BLACK, 383);
            let trace = format!("{}: {}", device_label(device), describe(&result));
            // Axis-aligned lines on pixel centres have coverage 1 on their
            // pixels and 0 beside them, so they agree exactly. The diagonal is
            // 40 * sqrt(2) = 57 px long, and a 1 px line at 45 degrees leaves
            // each pixel it crosses at up to half coverage: allow all of its
            // 57 edge-hugging pixels on each side to flip, 114, plus the
            // point's 20 px outline.
            assert!(result.coverage_mismatches <= 134, "{trace}");
            // The lines drawn: the 1 px line's 81 + 2 caps, the 3 px line's 3
            // x 51 plus its caps: something is drawn on both.
            assert!(result.covered_b > 250, "{trace}");
            // The horizontal line's middle pixel is fully covered on both.
            let middle = 31 * WIDTH as usize + 60;
            assert_eq!(cpu_image[middle], white, "{trace}");
            assert!(brightness(gpu_image[middle]) >= 760, "{trace}");
        }
    }

    #[test]
    fn a_scene_at_mga_coordinates_draws_like_the_same_scene_at_the_origin() {
        const WIDTH: i32 = 160;
        const HEIGHT: i32 = 120;
        for device in DEVICES {
            let Some(mut gpu) = gpu_or_skip(WIDTH, HEIGHT, device) else {
                continue;
            };
            let home = Vec3::new(0.0, 0.0, 0.0);
            let survey = Vec3::new(300000.0, 6250000.0, 50.0);

            gpu.renderer().set_draw_list(&site(home));
            let at_home = render(
                &mut gpu,
                &site_camera(home, WIDTH, HEIGHT),
                &black_background(),
            );
            gpu.renderer().set_draw_list(&site(survey));
            let at_survey = render(
                &mut gpu,
                &site_camera(survey, WIDTH, HEIGHT),
                &black_background(),
            );
            save_for_looking(&label("rtc_home", device), &at_home, WIDTH, HEIGHT);
            save_for_looking(&label("rtc_survey", device), &at_survey, WIDTH, HEIGHT);

            let same = compare(&at_home, &at_survey, WIDTH, HEIGHT, BLACK, 128);
            let trace = format!("{}: {}", device_label(device), describe(&same));
            assert!(same.covered_a > 4000, "{trace}");
            // Relative to their own centres the two lists' positions are equal
            // to within a double's rounding at 6.25e6 (1e-9 m), far below a
            // float's resolution at 20 m (2e-6 m), so the GPU sees the same
            // floats, and the eye-relative matrices agree as closely. At most a
            // pixel or two on an edge may flip where a coordinate lands on a
            // rounding boundary.
            assert!(same.differing_pixels <= 4, "{trace}");
            assert!(same.coverage_mismatches <= 4, "{trace}");

            // The control: the survey scene packed against a zero origin, as a
            // GPU path without the rule would upload it, is visibly wrong -
            // its vertices snap to the half-metre float lattice at northing
            // 6.25e6.
            let mut raw = GpuSceneData::default();
            pack_draw_list_with_origin(&site(survey), Vec3::new(0.0, 0.0, 0.0), &mut raw);
            gpu.renderer().set_scene(raw);
            let without_origin = render(
                &mut gpu,
                &site_camera(survey, WIDTH, HEIGHT),
                &black_background(),
            );
            save_for_looking(
                &label("rtc_without_origin", device),
                &without_origin,
                WIDTH,
                HEIGHT,
            );
            let broken = compare(&at_home, &without_origin, WIDTH, HEIGHT, BLACK, 128);
            assert!(
                broken.differing_pixels > same.covered_a / 20,
                "{}: {}: the control should differ widely",
                device_label(device),
                describe(&broken)
            );
        }
    }

    // The upload counter is upload_count(): frames that uploaded anything,
    // as the C++'s uploadCount() counted them.
    #[test]
    fn a_frame_that_only_moves_the_camera_uploads_nothing() {
        for device in DEVICES {
            let Some(mut gpu) = gpu_or_skip(64, 48, device) else {
                continue;
            };
            let name = device_label(device);
            gpu.renderer().set_draw_list(&site(Vec3::default()));
            let mut camera = site_camera(Vec3::default(), 64, 48);
            let first = frame(&mut gpu, &camera, &black_background());
            assert!(first.uploaded, "{name}");
            assert_eq!(gpu.renderer().upload_count(), 1, "{name}");

            camera.orbit(0.3, 0.1);
            let second = frame(&mut gpu, &camera, &black_background());
            assert!(!second.uploaded, "{name}");
            assert_eq!(second.uploaded_bytes, 0, "{name}");
            assert_eq!(gpu.renderer().upload_count(), 1, "{name}");

            gpu.renderer()
                .set_draw_list(&site(Vec3::new(1.0, 0.0, 0.0)));
            let third = frame(&mut gpu, &camera, &black_background());
            assert!(third.uploaded, "{name}");
            assert_eq!(gpu.renderer().upload_count(), 2, "{name}");
        }
    }

    #[test]
    fn vertical_exaggeration_in_the_shader_draws_what_an_exaggerated_list_draws() {
        const WIDTH: i32 = 160;
        const HEIGHT: i32 = 120;
        const FACTOR: f64 = 3.0;
        const DATUM: f64 = -1.0;
        for device in DEVICES {
            let Some(mut gpu) = gpu_or_skip(WIDTH, HEIGHT, device) else {
                continue;
            };
            let flat = site(Vec3::default());
            let mut lifted = flat.clone();
            for position in &mut lifted.positions {
                position.z = DATUM + (position.z - DATUM) * FACTOR;
            }
            let camera = site_camera(Vec3::default(), WIDTH, HEIGHT);

            gpu.renderer().set_draw_list(&lifted);
            let expected = render(&mut gpu, &camera, &black_background());
            gpu.renderer().set_draw_list(&flat);
            let settings = FrameSettings {
                vertical_exaggeration: FACTOR,
                exaggeration_datum: DATUM,
                ..black_background()
            };
            let exaggerated = render(&mut gpu, &camera, &settings);
            save_for_looking(&label("exaggeration", device), &exaggerated, WIDTH, HEIGHT);

            let result = compare(&expected, &exaggerated, WIDTH, HEIGHT, BLACK, 128);
            let trace = format!("{}: {}", device_label(device), describe(&result));
            assert!(result.covered_a > 4000, "{trace}");
            // The two lists are packed against different origins (the lifted
            // one's box is taller), so the floats differ in their last bits: a
            // handful of edge pixels may flip, nothing more.
            assert!(result.coverage_mismatches <= 16, "{trace}");
            assert!(result.max_interior_difference <= 2, "{trace}");
        }
    }

    #[test]
    fn per_pixel_lighting_darkens_the_side_of_a_face_turned_from_the_light() {
        const SIZE: i32 = 48;
        for device in DEVICES {
            let Some(mut gpu) = gpu_or_skip(SIZE, SIZE, device) else {
                continue;
            };
            let name = device_label(device);
            let mut list = DrawList::default();
            add_quad(
                &mut list,
                [
                    Vec3::new(-20.0, -20.0, 0.0),
                    Vec3::new(20.0, -20.0, 0.0),
                    Vec3::new(20.0, 20.0, 0.0),
                    Vec3::new(-20.0, 20.0, 0.0),
                ],
                rgba(200, 200, 200),
            );
            gpu.renderer().set_draw_list(&list);

            let settings = FrameSettings {
                lighting: LightingMode::PerPixel,
                // 60 degrees from the zenith, so n.l = cos 60 = 0.5 for the
                // upper side.
                light_direction: Vec3::new((PI / 3.0).sin(), 0.0, (PI / 3.0).cos()),
                ambient: 0.35,
                ..black_background()
            };

            let above = plan_camera(SIZE, SIZE);
            let from_above = render(&mut gpu, &above, &settings);
            let mut below = above;
            below.set_standard_view(StandardView::Bottom);
            let from_below = render(&mut gpu, &below, &settings);

            let centre = (SIZE / 2) as usize * SIZE as usize + (SIZE / 2) as usize;
            // Above: 0.35 + 0.65 * 0.5 = 0.675 of 200 = 135.
            near_because(f64::from(red_of(from_above[centre])), 135.0, 2.0, name);
            // Below, the visible side faces away from the light: n.l = -0.5,
            // clamped to 0, ambient alone: 0.35 of 200 = 70. (A baked abs(n.l)
            // would give 135 again - both sides lit alike.)
            near_because(f64::from(red_of(from_below[centre])), 70.0, 2.0, name);
        }
    }

    #[test]
    fn a_line_on_the_ground_behind_a_wall_stays_hidden() {
        const WIDTH: i32 = 128;
        const HEIGHT: i32 = 96;
        for device in DEVICES {
            let Some(mut gpu) = gpu_or_skip(WIDTH, HEIGHT, device) else {
                continue;
            };
            let name = device_label(device);
            // A 40 x 25 m red wall facing south, and 5 m north of it a white
            // line on the ground with the scene builder's entity bias, which
            // in the software path's standard-Z buffer is enough to pull it
            // through the wall.
            let mut list = red_wall();
            list.add_segment(
                Vec3::new(-10.0, 5.0, 0.0),
                Vec3::new(10.0, 5.0, 0.0),
                rgba(255, 255, 255),
                1.0,
                2.0e-4,
            );
            let mut camera = Camera::default();
            camera.set_viewport_size(WIDTH, HEIGHT);
            camera.set_projection(Projection::Perspective);
            // From the south, a little above.
            camera.set_orientation(-PI / 2.0, 0.2);
            assert!(camera.frame(list.bounds()));

            // Anything with green in it has the line in it: the wall is pure
            // red and the ground black.
            let line_on =
                |image: &[Rgba]| image.iter().filter(|&&pixel| green_of(pixel) > 8).count();
            gpu.renderer().set_draw_list(&list);
            let image = render(&mut gpu, &camera, &black_background());
            save_for_looking(&label("xray", device), &image, WIDTH, HEIGHT);
            assert_eq!(line_on(&image), 0, "{name}");

            // The control: without the wall the line is there to be seen. The
            // camera is unchanged, so it is the wall, not the framing, that
            // hid it.
            let mut line_only = list.clone();
            line_only.triangles.clear();
            gpu.renderer().set_draw_list(&line_only);
            let control = render(&mut gpu, &camera, &black_background());
            assert!(line_on(&control) > 10, "{name}");
        }
    }

    // A mark lying on a surface draws whole over it. The quad a point or a
    // line becomes is flat in depth - the depth of its centre - while the
    // surface under it slopes away, so wherever the quad reaches uphill of its
    // centre the surface is nearer than the quad. A size-5 point reaches 3 px
    // from its centre (3.6 px at 125%), well past what the fill's offset of
    // two pixels of its own depth slope covers, and before the marks were
    // pulled towards the eye (gpu_renderer, MARK_PULL) lost the uphill corner
    // of every square: 13% of the points' pixels here, 17.5% at 125%.
    #[test]
    fn marks_lying_on_a_sloped_surface_draw_whole_over_it() {
        const WIDTH: i32 = 256;
        const HEIGHT: i32 = 192;
        for device in DEVICES {
            let Some(mut gpu) = gpu_or_skip(WIDTH, HEIGHT, device) else {
                continue;
            };
            let name = device_label(device);
            // 35 degrees down from the south-west. The relief's steepest facet
            // climbs 24 degrees (dz/dx at most 3 * 0.15 = 0.45 = tan 24.2),
            // less than the view's 35, so no facet turns its back on the eye
            // and nothing on the TIN is hidden by the TIN: every mark pixel
            // drawn on black must still show over the surface.
            let mut camera = Camera::default();
            camera.set_viewport_size(WIDTH, HEIGHT);
            camera.set_projection(Projection::Perspective);
            camera.set_orientation(-3.0 * PI / 4.0, 35.0 * PI / 180.0);
            let mut framing = DrawList::default();
            add_sloped_surface(&mut framing);
            assert!(camera.frame(framing.bounds()));

            // Every mark pixel, less the odd one where the antialiased edge of
            // the quad meets a triangle's edge in the depth test's last bit.
            let expect_whole = |kept: Kept, what: &str| {
                assert!(
                    kept.marks > 500,
                    "{name}, {what}: {} mark pixels",
                    kept.marks
                );
                assert!(
                    kept.shown as f64 >= 0.995 * kept.marks as f64,
                    "{name}, {what}: {} of {} mark pixels show over the surface",
                    kept.shown,
                    kept.marks
                );
            };
            expect_whole(
                kept_over(
                    &mut gpu,
                    device,
                    &slope_points(5.0),
                    1.0,
                    "slope_points",
                    &camera,
                ),
                "size-5 points",
            );
            expect_whole(
                kept_over(
                    &mut gpu,
                    device,
                    &slope_points(5.0),
                    1.25,
                    "slope_points_125",
                    &camera,
                ),
                "size-5 points at 125%",
            );
            expect_whole(
                kept_over(
                    &mut gpu,
                    device,
                    &slope_rows(4.0),
                    1.0,
                    "slope_lines4",
                    &camera,
                ),
                "4 px lines",
            );
            expect_whole(
                kept_over(
                    &mut gpu,
                    device,
                    &slope_rows(2.0),
                    1.25,
                    "slope_lines2_125",
                    &camera,
                ),
                "2 px lines at 125%",
            );
            expect_whole(
                kept_over(
                    &mut gpu,
                    device,
                    &slope_rows(1.0),
                    1.0,
                    "slope_lines1",
                    &camera,
                ),
                "1 px lines, which the fill's own offset already covered",
            );
            // The same view without perspective: the pull is then a fixed
            // length along the view direction rather than a share of the depth.
            let mut orthographic = camera;
            orthographic.set_projection(Projection::Orthographic);
            expect_whole(
                kept_over(
                    &mut gpu,
                    device,
                    &slope_points(5.0),
                    1.0,
                    "slope_points_ortho",
                    &orthographic,
                ),
                "size-5 points, orthographic",
            );
        }
    }

    // The other side of the same pull: a mark pulled towards the eye so that
    // the surface it lies on cannot cut it must still be hidden by a wall
    // standing well in front of it.
    #[test]
    fn a_point_well_behind_a_wall_stays_hidden() {
        const WIDTH: i32 = 128;
        const HEIGHT: i32 = 96;
        for device in DEVICES {
            let Some(mut gpu) = gpu_or_skip(WIDTH, HEIGHT, device) else {
                continue;
            };
            let name = device_label(device);
            let mut list = red_wall();
            let mut camera = Camera::default();
            camera.set_viewport_size(WIDTH, HEIGHT);
            camera.set_projection(Projection::Perspective);
            // From the south, a little above.
            camera.set_orientation(-PI / 2.0, 0.2);
            assert!(camera.frame(list.bounds()));
            // A 9 px point - larger than the scene builder's 5, so pulled
            // further: it reaches 4.5 + 0.5 = 5 px, and is pulled 3 x (5 - 1)
            // = 12 pixels' worth of world (gpu_renderer, MARK_PULL) - twice
            // its own size, 18 pixels' worth, behind the wall, at the wall's
            // metres per pixel.
            let metres_per_pixel = camera.world_per_pixel();
            let behind = 2.0 * 9.0 * metres_per_pixel;
            let mark = list.add_vertex(Vec3::new(0.0, behind, 12.0), MARK_COLOUR);
            list.add_point(mark, 9.0, 0.0);

            let mark_on =
                |image: &[Rgba]| image.iter().filter(|&&pixel| blue_of(pixel) > 8).count();
            gpu.renderer().set_draw_list(&list);
            let image = render(&mut gpu, &camera, &black_background());
            save_for_looking(&label("point_behind_wall", device), &image, WIDTH, HEIGHT);
            assert_eq!(mark_on(&image), 0, "{name}");

            // The control: without the wall the point is there to be seen.
            let mut point_only = list.clone();
            point_only.triangles.clear();
            gpu.renderer().set_draw_list(&point_only);
            let control = render(&mut gpu, &camera, &black_background());
            assert!(mark_on(&control) > 40, "{name}");
        }
    }

    #[test]
    fn zooming_far_out_never_clips_the_scene_away() {
        const SIZE: i32 = 64;
        for device in DEVICES {
            let Some(mut gpu) = gpu_or_skip(SIZE, SIZE, device) else {
                continue;
            };
            let name = device_label(device);
            let list = site(Vec3::default());
            let mut camera = Camera::default();
            camera.set_viewport_size(SIZE, SIZE);
            camera.set_standard_view(StandardView::IsoSouthWest);
            assert!(camera.frame(list.bounds()));
            // Past the far plane frame() chose (at most 8 radii beyond the
            // target): 20 notches of the wheel out is 1.15^20 = 16x the
            // distance.
            camera.dolly(1.15f64.powf(20.0));
            assert!(camera.distance() > camera.far_plane(), "{name}");
            gpu.renderer().set_draw_list(&list);
            let image = render(&mut gpu, &camera, &black_background());
            let drawn = image.iter().filter(|&&pixel| brightness(pixel) > 0).count();
            assert!(drawn > 0, "{name}");
        }
    }

    // A cloud point is a round sprite of FrameSettings::cloud_point_size
    // logical pixels whatever its distance, drawn relative to the cloud's own
    // origin.
    #[test]
    fn cloud_points_are_round_sprites_of_their_screen_size_at_survey_coordinates() {
        const WIDTH: i32 = 64;
        const HEIGHT: i32 = 48;
        for device in DEVICES {
            let Some(mut gpu) = gpu_or_skip(WIDTH, HEIGHT, device) else {
                continue;
            };
            let name = device_label(device);
            // Three points in a row at MGA coordinates, 16 pixels apart in the
            // plan camera moved there, each on a pixel centre (the camera's
            // half-integer grid).
            let survey = Vec3::new(300000.0, 6250000.0, 50.0);
            let points = [
                survey + Vec3::new(-15.5, 0.5, 0.0),
                survey + Vec3::new(0.5, 0.5, 0.0),
                survey + Vec3::new(16.5, 0.5, 0.0),
            ];
            let mut cloud = PointCloudData::default();
            pack_point_cloud(&points, &[], rgba(255, 255, 255), survey, 0, &mut cloud);
            gpu.renderer().set_point_cloud(cloud);
            let mut camera = plan_camera(WIDTH, HEIGHT);
            camera.set_target(survey);
            let settings = FrameSettings {
                cloud_point_size: 6.0,
                ..black_background()
            };
            let image = render(&mut gpu, &camera, &settings);
            save_for_looking(&label("cloud", device), &image, WIDTH, HEIGHT);

            // A disc of radius 3 around each point's pixel. Its coverage fades
            // over the pixel either side of that radius, so at least half
            // coverage means a centre within 3 px: pi * 3^2 = 28.3 px of area,
            // which a pixel grid fills with between the 21 whole pixels inside
            // radius 2.6 and the 37 inside radius 3.4 (counted by hand on the
            // grid).
            let disc_at = |cx: i32, cy: i32| {
                let mut count = 0usize;
                for y in cy - 5..=cy + 5 {
                    for x in cx - 5..=cx + 5 {
                        let pixel = image[y as usize * WIDTH as usize + x as usize];
                        count += usize::from(brightness(pixel) >= 383);
                    }
                }
                count
            };
            // World (x, 0.5) is pixel column x + 32 - 0.5, row 23.
            for column in [16, 32, 48] {
                let centre = image[23 * WIDTH as usize + column as usize];
                assert!(brightness(centre) >= 760, "{name}: column {column}");
                let disc = disc_at(column, 23);
                assert!(disc >= 21, "{name}: column {column}: {disc}");
                assert!(disc <= 37, "{name}: column {column}: {disc}");
            }
        }
    }
}

mod shader_compiler {
    use super::*;

    // Every module the renderer draws with, checked by naga as wgpu checks
    // it: the twin of compiling every HLSL stage of both expansions. wgpu has
    // one expansion, instancing.
    #[test]
    fn every_stage_of_both_expansions_compiles() {
        for expansion in ALL_EXPANSIONS {
            if let Err(error) = precompile_shaders(expansion) {
                panic!("{}", error.describe());
            }
        }
    }

    #[test]
    fn reports_a_compile_error_with_the_compilers_own_message() {
        let error = compile_wgsl(
            "@fragment fn fs_main() -> @location(0) vec4<f32> { return undefinedColour; }",
        )
        .expect_err("a module naming nothing that exists must not compile");
        let message = error.describe();
        assert!(message.contains("did not compile"), "{message}");
        assert!(message.contains("undefinedColour"), "{message}");
    }
}
