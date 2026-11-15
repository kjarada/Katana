//! A scene of layers on the GPU (GpuRenderer::set_layers): each drawn whole, in
//! order, into one depth buffer, with its own depth rule - the GPU twin of the
//! cad layer's render_layers, which the 3D view draws its grid, terrain, edges,
//! drawing and selection with (its scene module has why the grid and the
//! edges write no depth). Every case draws the scene both ways - the layer
//! writing depth and not - so it shows the rule is what makes the difference.
//! The twin of tests/gpu/test_gpu_layers.cpp.

use crate::gpu_support::{
    DEVICES, Image, brightness, device_label, gpu_or_skip, label, save_for_looking,
};
use katana_gpu::{
    FrameSettings, GpuLine, GpuVertex, LayerSource, ORIGIN_ERROR_PIXELS, OffscreenGpu,
    origin_error_pixels,
};
use katana_math::Vec3;
use katana_render::{
    Camera, DrawList, Projection, Rgba, StandardView, blue_of, green_of, red_of, rgba,
};

const WIDTH: i32 = 64;
const HEIGHT: i32 = 48;

// Straight down, orthographic, one world unit per pixel: the centre of pixel
// (column, row) is world (column + 0.5 - 32, 23.5 - row), so (32, 23) is
// world (0.5, 0.5).
fn plan_camera() -> Camera {
    let mut camera = Camera::default();
    camera.set_viewport_size(WIDTH, HEIGHT);
    camera.set_projection(Projection::Orthographic);
    camera.set_standard_view(StandardView::Top);
    camera.set_target(Vec3::new(0.0, 0.0, 0.0));
    camera.set_distance(1000.0);
    camera.set_orthographic_height(f64::from(HEIGHT));
    camera.set_depth_range(1.0, 2000.0);
    camera
}

// A flat green triangle at z = 0 over the middle of the view: a pad, a pond
// floor - a surface exactly in the plane of whatever else lies at z = 0.
fn flat_surface() -> DrawList {
    let mut list = DrawList::default();
    let green = rgba(0, 200, 0);
    let a = list.add_vertex(Vec3::new(-20.0, -15.0, 0.0), green);
    let b = list.add_vertex(Vec3::new(20.0, -15.0, 0.0), green);
    let c = list.add_vertex(Vec3::new(0.0, 20.0, 0.0), green);
    list.add_triangle(a, b, c);
    list
}

fn line_at(a: Vec3, b: Vec3, color: Rgba, width: f32) -> DrawList {
    let mut list = DrawList::default();
    list.add_segment(a, b, color, width, 0.0);
    list
}

fn pixel_at(image: &[Rgba], column: i32, row: i32) -> Rgba {
    image[row as usize * WIDTH as usize + column as usize]
}

fn black_background() -> FrameSettings {
    FrameSettings {
        background: rgba(0, 0, 0),
        ..FrameSettings::default()
    }
}

// The C++ fixture's render(): the layers set, one frame through the plan
// camera, read back.
fn render(gpu: &mut OffscreenGpu, layers: &[LayerSource<'_>]) -> Image {
    gpu.renderer().set_layers(layers, None);
    let mut pixels = Vec::new();
    if let Err(error) = gpu.render_frame(&plan_camera(), &black_background(), Some(&mut pixels)) {
        panic!("{}", error.describe());
    }
    pixels
}

fn layer(list: &DrawList, depth_write: bool) -> LayerSource<'_> {
    LayerSource {
        list: Some(list),
        depth_write,
    }
}

mod gpu_layers {
    use super::*;

    // The grid is a backdrop: drawn first and writing no depth, it is covered
    // by a surface lying exactly in its plane. Writing depth, the grid won the
    // tie - the flat surface's own polygon offset pushes it back - and showed
    // through the pad (docs/render.md, "The grid drawn across a flat pad").
    #[test]
    fn a_grid_that_writes_no_depth_stays_under_a_surface_in_its_plane() {
        for device in DEVICES {
            let Some(mut gpu) = gpu_or_skip(WIDTH, HEIGHT, device) else {
                continue;
            };
            let name = device_label(device);
            let grid = line_at(
                Vec3::new(-30.5, 0.5, 0.0),
                Vec3::new(30.5, 0.5, 0.0),
                rgba(255, 255, 255),
                1.0,
            );
            let surface = flat_surface();

            let backdrop = [layer(&grid, false), layer(&surface, true)];
            let drawn = render(&mut gpu, &backdrop);
            save_for_looking(
                &label("layers_grid_backdrop", device),
                &drawn,
                WIDTH,
                HEIGHT,
            );
            // Under the surface: the surface's green, not the grid's white.
            let under = pixel_at(&drawn, 32, 23);
            assert!(green_of(under) > 150, "{name}");
            assert!(red_of(under) < 60, "{name}");
            // Beside it the grid is there: nothing covers it.
            assert!(brightness(pixel_at(&drawn, 4, 23)) > 600, "{name}");

            // The control: the same grid writing depth shows through.
            let written = [layer(&grid, true), layer(&surface, true)];
            let control = render(&mut gpu, &written);
            assert!(
                red_of(pixel_at(&control, 32, 23)) > 150,
                "{name}: without the rule the grid is expected to win the tie; if it no longer \
                 does, this case no longer tests the rule"
            );
        }
    }

    // A surface's edges are tested against it and write no depth, so a line
    // of the drawing lying in the same surface draws whole across them.
    // Writing depth, the edge drawn first won every tie, and a draped string
    // was dashed wherever a TIN edge crossed it (docs/render.md). Both lines
    // are 1 px, the drawing's own width (the scene builder's entity line
    // width): a wider mark is pulled towards the eye by its size
    // (gpu_renderer, MARK_PULL) and wins whatever the edges write, so it
    // would not test the rule.
    #[test]
    fn edges_that_write_no_depth_do_not_break_a_line_drawn_over_them() {
        for device in DEVICES {
            let Some(mut gpu) = gpu_or_skip(WIDTH, HEIGHT, device) else {
                continue;
            };
            let name = device_label(device);
            let surface = flat_surface();
            let edge = line_at(
                Vec3::new(0.5, -10.5, 0.0),
                Vec3::new(0.5, 10.5, 0.0),
                rgba(90, 90, 90),
                1.0,
            );
            let drawing = line_at(
                Vec3::new(-10.5, 0.5, 0.0),
                Vec3::new(10.5, 0.5, 0.0),
                rgba(255, 0, 0),
                1.0,
            );

            let layered = [
                layer(&surface, true),
                layer(&edge, false),
                layer(&drawing, true),
            ];
            let drawn = render(&mut gpu, &layered);
            save_for_looking(&label("layers_edges", device), &drawn, WIDTH, HEIGHT);
            // Where the two cross, the drawing's red.
            let crossing = pixel_at(&drawn, 32, 23);
            assert!(red_of(crossing) > 200, "{name}");
            assert!(green_of(crossing) < 60, "{name}");
            // Along the edge away from the line, the edge is drawn over the
            // surface.
            let along = pixel_at(&drawn, 32, 13);
            assert!(
                green_of(along) < 150,
                "{name}: the edge covers the surface there"
            );

            let written = [
                layer(&surface, true),
                layer(&edge, true),
                layer(&drawing, true),
            ];
            let control = render(&mut gpu, &written);
            assert!(
                red_of(pixel_at(&control, 32, 23)) < 200,
                "{name}: without the rule the edge is expected to win the tie; if it no longer \
                 does, this case no longer tests the rule"
            );
        }
    }

    // One layer changing alone - a selection, edges recoloured by their fade -
    // is uploaded alone, against the origin the scene was packed with, and
    // keeps its depth rule.
    #[test]
    fn a_layer_updated_alone_is_the_only_one_uploaded() {
        for device in DEVICES {
            let Some(mut gpu) = gpu_or_skip(WIDTH, HEIGHT, device) else {
                continue;
            };
            let name = device_label(device);
            let surface = flat_surface();
            let grid = line_at(
                Vec3::new(-30.5, 0.5, 0.0),
                Vec3::new(30.5, 0.5, 0.0),
                rgba(255, 255, 255),
                1.0,
            );
            let layers = [layer(&grid, false), layer(&surface, true)];
            gpu.renderer().set_layers(&layers, None);
            let first = match gpu.render_frame(&plan_camera(), &black_background(), None) {
                Ok(stats) => stats,
                Err(error) => panic!("{name}: {}", error.describe()),
            };
            assert!(first.uploaded, "{name}");
            let origin = gpu.renderer().scene().origin;

            // The grid moved down to row 33 (world y = -9.5), still passing
            // under the surface; the surface is not sent again.
            let moved = line_at(
                Vec3::new(-30.5, -9.5, 0.0),
                Vec3::new(30.5, -9.5, 0.0),
                rgba(255, 255, 255),
                1.0,
            );
            gpu.renderer().update_layer(0, &moved);
            let mut pixels = Vec::new();
            let second =
                match gpu.render_frame(&plan_camera(), &black_background(), Some(&mut pixels)) {
                    Ok(stats) => stats,
                    Err(error) => panic!("{name}: {}", error.describe()),
                };
            assert!(second.uploaded, "{name}");
            assert_eq!(
                second.uploaded_bytes,
                2 * std::mem::size_of::<GpuVertex>() + std::mem::size_of::<GpuLine>(),
                "{name}"
            );
            assert_eq!(gpu.renderer().layer_count(), 2, "{name}");
            assert_eq!(gpu.renderer().scene().origin, origin, "{name}");
            assert!(brightness(pixel_at(&pixels, 4, 33)) > 600, "{name}");
            // Still a backdrop: under the surface the moved grid does not show.
            assert!(red_of(pixel_at(&pixels, 32, 33)) < 60, "{name}");
            assert!(green_of(pixel_at(&pixels, 32, 33)) > 150, "{name}");

            // A frame that changes nothing uploads nothing.
            let third = match gpu.render_frame(&plan_camera(), &black_background(), None) {
                Ok(stats) => stats,
                Err(error) => panic!("{name}: {}", error.describe()),
            };
            assert!(!third.uploaded, "{name}");
        }
    }

    // Zoomed in close to a survey far from its scene's centre, the float
    // offsets packed against that centre round to more than the view is wide;
    // packed against the pivot, as the 3D view packs them once the bound there
    // passes ORIGIN_ERROR_PIXELS (scene_origin, "The origin follows a deep
    // zoom"), the survey is drawn where it is. One stray point at the origin
    // puts the centre of the scene's box 3100 km from a line at MGA
    // coordinates, where a float steps in 0.25 m; the view is 48 mm tall, a
    // millimetre a pixel, and the line runs 10.5 mm north of the pivot: along
    // the centre of row 23.5 - 10.5 = 13.
    #[test]
    fn layers_packed_against_the_pivot_draw_a_survey_zoomed_in_close_where_it_is() {
        for device in DEVICES {
            let Some(mut gpu) = gpu_or_skip(WIDTH, HEIGHT, device) else {
                continue;
            };
            let name = device_label(device);
            let pivot = Vec3::new(300000.3, 6200000.3, 30.0);
            let line = line_at(
                pivot + Vec3::new(-0.04, 0.0105, 0.0),
                pivot + Vec3::new(0.04, 0.0105, 0.0),
                rgba(255, 255, 255),
                1.0,
            );
            let mut stray = DrawList::default();
            let point = stray.add_vertex(Vec3::new(0.0, 0.0, 30.0), rgba(255, 255, 255));
            stray.add_point(point, 3.0, 0.0);
            let layers = [layer(&stray, true), layer(&line, true)];
            let mut camera = plan_camera();
            camera.set_target(pivot);
            camera.set_orthographic_height(0.048);

            gpu.renderer().set_layers(&layers, None);
            assert!(
                origin_error_pixels(&camera, gpu.renderer().origin()) > ORIGIN_ERROR_PIXELS,
                "{name}: the scene's centre is near enough to the pivot, so this proves nothing"
            );
            let mut far_image = Vec::new();
            if let Err(error) = gpu.render_frame(&camera, &black_background(), Some(&mut far_image))
            {
                panic!("{name}: {}", error.describe());
            }
            save_for_looking(&label("far_origin", device), &far_image, WIDTH, HEIGHT);
            assert!(
                brightness(pixel_at(&far_image, 32, 13)) < 100,
                "{name}: drawn in its place from 3100 km off, so this proves nothing"
            );

            gpu.renderer().set_layers(&layers, Some(pivot));
            assert_eq!(gpu.renderer().origin(), pivot, "{name}");
            assert!(
                origin_error_pixels(&camera, pivot) <= ORIGIN_ERROR_PIXELS,
                "{name}"
            );
            let mut near_image = Vec::new();
            if let Err(error) =
                gpu.render_frame(&camera, &black_background(), Some(&mut near_image))
            {
                panic!("{name}: {}", error.describe());
            }
            save_for_looking(&label("pivot_origin", device), &near_image, WIDTH, HEIGHT);
            for column in [4, 32, 60] {
                assert!(
                    brightness(pixel_at(&near_image, column, 13)) > 600,
                    "{name}: {column}"
                );
                assert!(
                    brightness(pixel_at(&near_image, column, 11)) < 100,
                    "{name}: {column}"
                );
                assert!(
                    brightness(pixel_at(&near_image, column, 15)) < 100,
                    "{name}: {column}"
                );
            }
        }
    }

    // The 3D view's selection (the cad layer's selection style): a 3 px core
    // of #FF9F1C over a 5 px casing of (16, 18, 22), both over the entity's
    // own 1 px line. The GPU pulls a mark towards the eye by its size
    // (gpu_renderer, MARK_PULL) and reads no draw-list bias, so in ONE layer
    // writing depth the wider casing is nearer than the core wherever they
    // overlap and covers it, whichever is drawn first: the selection was a
    // dark line. In a layer of its own, before the core's and writing no
    // depth, the casing is covered by the core drawn after it and shows only
    // either side (the scene's layers).
    #[test]
    fn a_selection_core_drawn_after_a_casing_that_writes_no_depth_keeps_its_colour() {
        for device in DEVICES {
            let Some(mut gpu) = gpu_or_skip(WIDTH, HEIGHT, device) else {
                continue;
            };
            let name = device_label(device);
            let orange = rgba(0xFF, 0x9F, 0x1C);
            let dark = rgba(16, 18, 22);
            let from = Vec3::new(-10.5, 0.5, 0.0);
            let to = Vec3::new(10.5, 0.5, 0.0);
            let surface = flat_surface();
            let drawing = line_at(from, to, rgba(220, 220, 220), 1.0);
            let casing = line_at(from, to, dark, 5.0);
            let core = line_at(from, to, orange, 3.0);

            let layered = [
                layer(&surface, true),
                layer(&drawing, true),
                layer(&casing, false),
                layer(&core, true),
            ];
            let drawn = render(&mut gpu, &layered);
            save_for_looking(
                &label("layers_selection_casing", device),
                &drawn,
                WIDTH,
                HEIGHT,
            );
            // The line is on row 23 (world y = 0.5). The core covers rows
            // 22-24, the casing rows 21-25.
            let centre = pixel_at(&drawn, 32, 23);
            assert!(red_of(centre) > 220, "{name}");
            assert!(green_of(centre) > 120, "{name}");
            assert!(green_of(centre) < 190, "{name}");
            assert!(blue_of(centre) < 80, "{name}");
            // Two rows off the centre, inside the casing and outside the core:
            // dark, well under half the core's brightness (255 + 159 + 28 =
            // 442).
            for row in [21, 25] {
                assert!(
                    brightness(pixel_at(&drawn, 32, row)) < 221,
                    "{name}: row {row}"
                );
            }

            // The control: core then casing in one layer writing depth, as a
            // scene with no casing layer of its own would send them.
            let mut both = DrawList::default();
            both.add_segment(from, to, orange, 3.0, 0.0);
            both.add_segment(from, to, dark, 5.0, 0.0);
            let one_layer = [
                layer(&surface, true),
                layer(&drawing, true),
                layer(&both, true),
            ];
            let control = render(&mut gpu, &one_layer);
            assert!(
                brightness(pixel_at(&control, 32, 23)) < 221,
                "{name}: without the rule the casing is expected to cover the core; if it no \
                 longer does, this case no longer tests the rule"
            );
        }
    }
}
