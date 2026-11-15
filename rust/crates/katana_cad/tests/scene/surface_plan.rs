// A surface in plan, in its style (terrain/surface_plan.rs,
// docs/surface_display.md "In plan"; tests/cad/terrain/test_surface_plan.cpp):
// culling, the level of detail, the faces' image, contours, the rim and
// breaklines, points and labels - on hand-made TINs whose answers are worked
// out beside each test.

use std::sync::Arc;

use crate::{near, tin};
use katana_cad::terrain::surface_colouring::aspect_sector_colour;
use katana_cad::terrain::surface_plan::{
    SurfacePlan, SurfacePlanCache, SurfacePlanView, SurfaceProblem, contour_refusal,
    plan_image_grid, plan_triangles_in_box, plan_triangles_in_box_exhaustive,
    surface_style_problems,
};
use katana_cad::terrain::surface_style::{
    SurfaceContours, SurfaceDisplayStyle, SurfaceFaces, built_in_surface_styles,
};
use katana_core::task_pool::TaskPool;
use katana_entity::Color;
use katana_geometry::{Box2, Point2, Point3};
use katana_terrain::{NamedSurface, SurfaceStore, TinSurface};

/// The 10 x 10 square from (0, 0), z = x, as two triangles meeting on the
/// diagonal from (0, 0) to (10, 10); `breakline` makes that diagonal a
/// constrained edge (triangle 0's edge 2, triangle 1's edge 0).
fn square(breakline: bool) -> Arc<TinSurface> {
    let flags = if breakline {
        vec![0b100, 0b001]
    } else {
        Vec::new()
    };
    match TinSurface::create(
        vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(10.0, 0.0, 10.0),
            Point3::new(10.0, 10.0, 10.0),
            Point3::new(0.0, 10.0, 0.0),
        ],
        vec![[0, 1, 2], [0, 2, 3]],
        flags,
    ) {
        Ok(surface) => Arc::new(surface),
        Err(error) => panic!("{}", error.describe()),
    }
}

/// A grid of `columns` x `rows` cells of `cell`, each cut on its diagonal, z
/// rolling gently so no two faces are alike.
fn grid(columns: i32, rows: i32, cell: f64) -> Arc<TinSurface> {
    let mut vertices = Vec::new();
    for j in 0..=rows {
        for i in 0..=columns {
            let x = f64::from(i) * cell;
            let y = f64::from(j) * cell;
            vertices.push(Point3::new(
                x,
                y,
                10.0 + 3.0 * (x / 17.0).sin() * (y / 11.0).cos() + 0.05 * x,
            ));
        }
    }
    let at = |i: i32, j: i32| (j * (columns + 1) + i) as u32;
    let mut triangles = Vec::new();
    for j in 0..rows {
        for i in 0..columns {
            triangles.push([at(i, j), at(i + 1, j), at(i + 1, j + 1)]);
            triangles.push([at(i, j), at(i + 1, j + 1), at(i, j + 1)]);
        }
    }
    Arc::new(tin(vertices, triangles))
}

/// Looking at `visible` with `units_per_pixel`, the image on the plan view's
/// own grid (plan_image_grid, no cap that matters here).
fn view_of<'a>(visible: Box2, units_per_pixel: f64, bounds: Box2) -> SurfacePlanView<'a> {
    let grid = plan_image_grid(&visible, &bounds, units_per_pixel, 16_000_000);
    SurfacePlanView {
        visible,
        units_per_pixel,
        image_box: grid.r#box,
        image_width: grid.width,
        image_height: grid.height,
        ..SurfacePlanView::default()
    }
}

/// Nothing drawn but what each test turns on.
fn bare() -> SurfaceDisplayStyle {
    SurfaceDisplayStyle {
        name: String::from("bare"),
        show_boundary: false,
        plan_faces: SurfaceFaces::None,
        ..SurfaceDisplayStyle::default()
    }
}

fn pixel_at(plan: &SurfacePlan, column: i32, row: i32) -> u32 {
    plan.image.pixels[row as usize * plan.image.width as usize + column as usize]
}

const fn opaque(r: u8, g: u8, b: u8) -> u32 {
    0xff00_0000 | ((r as u32) << 16) | ((g as u32) << 8) | b as u32
}

fn square_box() -> Box2 {
    Box2::new(Point2::new(0.0, 0.0), Point2::new(10.0, 10.0))
}

fn boxed(x0: f64, y0: f64, x1: f64, y1: f64) -> Box2 {
    Box2::new(Point2::new(x0, y0), Point2::new(x1, y1))
}

/// The C++'s `{name, tin, "test"}`.
fn named(name: &str, surface: &Arc<TinSurface>) -> NamedSurface {
    NamedSurface {
        name: String::from(name),
        surface: Some(Arc::clone(surface)),
        source: String::from("test"),
        ..NamedSurface::default()
    }
}

fn plan_of(
    surface: &Arc<TinSurface>,
    style: &SurfaceDisplayStyle,
    view: &SurfacePlanView<'_>,
    cache: &mut SurfacePlanCache,
) -> SurfacePlan {
    katana_cad::terrain::surface_plan::surface_plan(&named("ground", surface), style, view, cache)
}

mod surface_plan_cull {
    use super::*;

    #[test]
    fn the_grid_finds_exactly_the_triangles_testing_every_one_finds() {
        let surface = grid(40, 30, 2.5); // 2 400 triangles over 100 x 75
        let mut cache = SurfacePlanCache::new();
        // A small linear congruential stream (Numerical Recipes' constants),
        // so the boxes are the same every run: inside, across the edge,
        // outside, tiny, thin, and the whole surface. Each draw is taken in
        // the order written; the C++ leaves the order of the four draws in one
        // box's corner to its compiler, which the properties below do not
        // depend on.
        let mut state: u32 = 12345;
        let mut next = || {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            f64::from(state >> 8) / f64::from(1u32 << 24)
        };
        let mut boxes = vec![
            boxed(-10.0, -10.0, 200.0, 200.0),
            boxed(50.0, 30.0, 50.0, 30.0),
            boxed(-5.0, 10.0, -1.0, 20.0),
            boxed(2.5, 2.5, 2.5, 70.0),
        ];
        for _ in 0..300 {
            let x = -20.0 + 140.0 * next();
            let y = -20.0 + 115.0 * next();
            let width = 40.0 * next() * next();
            let height = 30.0 * next() * next();
            boxes.push(boxed(x, y, x + width, y + height));
        }
        let mut non_empty = 0;
        for bounds in &boxes {
            let culled = plan_triangles_in_box(Some(&surface), bounds, &mut cache);
            let exhaustive = plan_triangles_in_box_exhaustive(&surface, bounds);
            assert_eq!(
                culled, exhaustive,
                "{},{} {},{}",
                bounds.min.x, bounds.min.y, bounds.max.x, bounds.max.y
            );
            non_empty += usize::from(!exhaustive.is_empty());
        }
        // The boxes reach the cases they claim: most find triangles, some
        // none.
        assert!(non_empty > boxes.len() / 2);
        assert!(non_empty < boxes.len());
        assert_eq!(cache.grids_built(), 1);
    }
}

mod surface_plan_triangles {
    use super::*;

    #[test]
    fn each_edge_at_least_four_pixels_long_is_stroked_once() {
        let surface = square(false);
        let mut cache = SurfacePlanCache::new();
        let style = SurfaceDisplayStyle {
            show_triangles: true,
            ..bare()
        };
        // A unit a pixel: sides 10 px, the diagonal 14 px - all stroked, the
        // shared diagonal once: 4 sides and 1 diagonal.
        let plan = plan_of(
            &surface,
            &style,
            &view_of(square_box(), 1.0, *surface.bounds()),
            &mut cache,
        );
        assert_eq!(plan.stats.triangles_in_view, 2);
        assert_eq!(plan.triangles.len(), 5);
        assert_eq!(plan.stats.triangles_tinted, 0);
        assert!(plan.image.empty()); // nothing to put in one
    }

    #[test]
    fn triangles_whose_edges_are_under_four_pixels_are_tinted_not_stroked() {
        let surface = square(false);
        let mut cache = SurfacePlanCache::new();
        let style = SurfaceDisplayStyle {
            show_triangles: true,
            triangle_colour: Color::new(150, 150, 150, 255),
            ..bare()
        };
        // Five units a pixel: sides 2 px, the diagonal 2.8 px - all under 4.
        let plan = plan_of(
            &surface,
            &style,
            &view_of(square_box(), 5.0, *surface.bounds()),
            &mut cache,
        );
        assert!(plan.triangles.is_empty());
        assert_eq!(plan.stats.triangles_tinted, 2);
        assert!(!plan.image.empty());
        // The tint: the triangle colour at 35 %, alpha lround(0.35 x 255) =
        // 89, each channel (150 x 89 + 127) / 255 = 52 (52.85 truncated).
        assert_eq!(pixel_at(&plan, 0, 0), 0x5934_3434);
    }
}

mod surface_plan_faces {
    use super::*;

    #[test]
    fn a_face_in_a_known_band_is_that_bands_colour() {
        let surface = square(false);
        let mut cache = SurfacePlanCache::new();
        let style = SurfaceDisplayStyle {
            plan_faces: SurfaceFaces::Elevation,
            plan_opacity: 1.0,
            banded: true,
            bands: 5, // edges at 2, 4, 6, 8 over the surface's own 0 to 10
            ramp: String::from("terrain"),
            ..bare()
        };
        // An eighth of a unit a pixel, exact in binary: 80 x 80 pixels, the
        // centre of column i at x = (i + 0.5) / 8, where z = x.
        let plan = plan_of(
            &surface,
            &style,
            &view_of(square_box(), 0.125, *surface.bounds()),
            &mut cache,
        );
        assert_eq!(plan.image.width, 80);
        assert_eq!(plan.image.height, 80);
        // Column 4: x = 0.5625, band 0, the ramp's start (40, 70, 140).
        assert_eq!(pixel_at(&plan, 4, 40), opaque(40, 70, 140));
        // Column 28: x = 3.5625, band 1 (2 to 4), the ramp at 1/4:
        // (60, 160, 90).
        assert_eq!(pixel_at(&plan, 28, 40), opaque(60, 160, 90));
        // Column 76: x = 9.5625, band 4, the ramp's end (245, 245, 245).
        assert_eq!(pixel_at(&plan, 76, 10), opaque(245, 245, 245));
        // Either side of the band edge at x = 2, columns 15 (1.9375) and 16
        // (2.0625), on rows of both triangles.
        for row in [5, 70] {
            assert_eq!(pixel_at(&plan, 15, row), opaque(40, 70, 140));
            assert_eq!(pixel_at(&plan, 16, row), opaque(60, 160, 90));
        }
        let legend = plan.legend.as_ref().expect("a legend");
        assert_eq!(legend.entries.len(), 5);
    }

    #[test]
    fn every_pixel_of_the_surface_is_covered_once_even_on_the_shared_diagonal() {
        // At an eighth of a unit a pixel, 80 of the pixel centres lie exactly
        // on the diagonal the two triangles share (i + j = 79 in the image's
        // rows and columns). Each goes to one triangle: no pinholes, none
        // twice - so exactly 80 x 80 covered. Counting a tie for both
        // triangles gives 6 480, for neither 6 320.
        let surface = square(false);
        let mut cache = SurfacePlanCache::new();
        let style = SurfaceDisplayStyle {
            plan_faces: SurfaceFaces::Flat,
            ..bare()
        };
        let plan = plan_of(
            &surface,
            &style,
            &view_of(square_box(), 0.125, *surface.bounds()),
            &mut cache,
        );
        assert_eq!(plan.stats.pixels_covered, 6400);
        for &pixel in &plan.image.pixels {
            assert_ne!(pixel, 0);
        }
    }

    #[test]
    fn opacity_premultiplies_the_flat_colour() {
        let surface = square(false);
        let mut cache = SurfacePlanCache::new();
        let style = SurfaceDisplayStyle {
            plan_faces: SurfaceFaces::Flat,
            flat_colour: Color::new(200, 100, 50, 255),
            plan_opacity: 0.5,
            ..bare()
        };
        let plan = plan_of(
            &surface,
            &style,
            &view_of(square_box(), 0.125, *surface.bounds()),
            &mut cache,
        );
        // Alpha lround(127.5) = 128; red (200 x 128 + 127) / 255 = 100, green
        // (100 x 128 + 127) / 255 = 50, blue (50 x 128 + 127) / 255 = 25.
        assert_eq!(pixel_at(&plan, 40, 40), 0x8064_3219);
    }

    #[test]
    fn slope_and_aspect_colour_a_face_by_its_steepness_and_direction() {
        // z = x rises 1 in 1 (100 %) towards the east, so every face looks
        // west: aspect 270 degrees, sector W.
        let surface = square(false);
        let mut cache = SurfacePlanCache::new();
        let mut style = SurfaceDisplayStyle {
            plan_opacity: 1.0,
            plan_faces: SurfaceFaces::Aspect,
            ..bare()
        };
        let aspect = plan_of(
            &surface,
            &style,
            &view_of(square_box(), 0.125, *surface.bounds()),
            &mut cache,
        );
        let west = aspect_sector_colour(6);
        assert_eq!(pixel_at(&aspect, 40, 40), opaque(west.r, west.g, west.b));
        // Slope over 0 to 200 % on the grey ramp: 100 % is its middle, (128,
        // 128, 128).
        style.plan_faces = SurfaceFaces::Slope;
        style.ramp = String::from("grey");
        style.range_min = Some(0.0);
        style.range_max = Some(200.0);
        let slope = plan_of(
            &surface,
            &style,
            &view_of(square_box(), 0.125, *surface.bounds()),
            &mut cache,
        );
        assert_eq!(pixel_at(&slope, 40, 40), opaque(128, 128, 128));
    }

    #[test]
    fn the_image_is_the_same_at_any_thread_count() {
        let surface = grid(60, 40, 1.7);
        let style = SurfaceDisplayStyle {
            plan_faces: SurfaceFaces::Elevation,
            banded: true,
            show_triangles: true, // dense at this scale: tinted
            ..bare()
        };
        let visible = boxed(-3.0, -2.0, 105.0, 70.0);
        let serial = TaskPool::new(0);
        let several = TaskPool::new(3);
        let mut cache_a = SurfacePlanCache::new();
        let mut cache_b = SurfacePlanCache::new();
        let mut view = view_of(visible, 0.3, *surface.bounds());
        view.pool = Some(&serial);
        let a = plan_of(&surface, &style, &view, &mut cache_a);
        view.pool = Some(&several);
        let b = plan_of(&surface, &style, &view, &mut cache_b);
        assert!(!a.image.empty());
        assert!(a.stats.pixels_covered > 0);
        assert_eq!(a.image.pixels, b.image.pixels);
        assert_eq!(a.stats.pixels_covered, b.stats.pixels_covered);
    }

    /// A fan of four triangles about the centre (4, 4), in the square from
    /// (0.25, 0.25) to (7.75, 7.75): every coordinate exact in binary, so the
    /// centre lands exactly on a pixel centre. `number` says which vertex
    /// index each point has: [0] the centre, then [1..4] the rim's
    /// south-west, south-east, north-east and north-west corners. The
    /// triangles are (centre, A, B) (centre, B, C) (centre, C, D) (centre, D,
    /// A), counter-clockwise.
    fn fan(number: [u32; 5]) -> Arc<TinSurface> {
        let mut vertices = vec![Point3::default(); 5];
        vertices[number[0] as usize] = Point3::new(4.0, 4.0, 12.0);
        vertices[number[1] as usize] = Point3::new(0.25, 0.25, 10.0);
        vertices[number[2] as usize] = Point3::new(7.75, 0.25, 10.0);
        vertices[number[3] as usize] = Point3::new(7.75, 7.75, 14.0);
        vertices[number[4] as usize] = Point3::new(0.25, 7.75, 14.0);
        let triangle = |a: usize, b: usize| [number[0], number[a], number[b]];
        Arc::new(tin(
            vertices,
            vec![
                triangle(1, 2),
                triangle(2, 3),
                triangle(3, 4),
                triangle(4, 1),
            ],
        ))
    }

    #[test]
    fn a_vertex_on_a_pixel_centre_is_one_triangles_whatever_the_vertex_numbers_are() {
        // Looking at -0.5 .. 7.5 a unit a pixel, the pixel centres are the
        // whole numbers 0 to 7 each way, and the fan's centre (4, 4) is
        // exactly one - as are the centres on the four lines from it to the
        // rim's corners ((3, 3), (5, 3), (5, 5) ...), which each go to one of
        // the two triangles sharing the line. The rim, 0.25 to 7.75, takes the
        // centres 1 to 7 each way: 7 x 7 = 49, each covered once.
        //
        // The rule that decided a tie by vertex numbers gave a centre on a
        // vertex to a triangle only if the vertex was numbered between its two
        // neighbours there: with the centre numbered first (0) or last (4)
        // every neighbour is above, or below, it, and no triangle had the
        // pixel - 48 covered, a pinhole; with the rim numbered 0, 3, 1, 4
        // about a centre numbered 2 the neighbours alternate and two triangles
        // had it - 50 writes for 49 pixels. (Worked out by running the old
        // rule over these four numberings by hand: 48, 48, 50 writes (49
        // pixels), 49.)
        let looking = boxed(-0.5, -0.5, 7.5, 7.5);
        let numberings: [[u32; 5]; 4] = [
            [0, 1, 2, 3, 4], // the centre below every neighbour
            [4, 0, 1, 2, 3], // the centre above every neighbour
            [2, 0, 3, 1, 4], // neighbours 0, 3, 1, 4 about it: above, below, above, below
            [2, 0, 1, 3, 4], // neighbours 0, 1, 3, 4: one run below, one above (the old rule's good case)
        ];
        for number in numberings {
            let surface = fan(number);
            let mut cache = SurfacePlanCache::new();
            let style = SurfaceDisplayStyle {
                plan_faces: SurfaceFaces::Flat,
                ..bare()
            };
            let plan = katana_cad::terrain::surface_plan::surface_plan(
                &named("fan", &surface),
                &style,
                &view_of(looking, 1.0, *surface.bounds()),
                &mut cache,
            );
            assert_eq!(plan.image.width, 8);
            assert_eq!(plan.image.height, 8);
            assert_eq!(
                plan.stats.pixels_covered, 49,
                "centre numbered {}",
                number[0]
            );
            // Pixel (i, j) has its centre at x = i, y = 7 - j.
            for j in 0..8 {
                for i in 0..8 {
                    let inside = i >= 1 && j <= 6;
                    assert_eq!(
                        pixel_at(&plan, i, j) != 0,
                        inside,
                        "pixel {i},{j} centre numbered {}",
                        number[0]
                    );
                }
            }
        }
    }

    #[test]
    fn a_range_above_the_whole_surface_colours_every_face_the_first_band_not_the_middle_one() {
        // The square z = x is 0 to 10; min=40 puts the range's low end at 40
        // and its top (the surface's 10) below it, a range given back to
        // front: the point 40. Every height is below the point - the first
        // band, the ramp's start (40, 70, 140) - and the middle band (235,
        // 220, 130) drew all of it.
        let surface = square(false);
        let mut cache = SurfacePlanCache::new();
        let style = SurfaceDisplayStyle {
            plan_faces: SurfaceFaces::Elevation,
            plan_opacity: 1.0,
            banded: true,
            bands: 5,
            range_min: Some(40.0),
            ..bare()
        };
        let plan = plan_of(
            &surface,
            &style,
            &view_of(square_box(), 0.125, *surface.bounds()),
            &mut cache,
        );
        assert_eq!(plan.image.width, 80);
        assert_eq!(pixel_at(&plan, 4, 40), opaque(40, 70, 140));
        assert_eq!(pixel_at(&plan, 76, 10), opaque(40, 70, 140));
    }

    #[test]
    fn a_level_surface_coloured_by_slope_is_the_ramps_start_not_its_middle() {
        // Slope's range is 0 to the steepest face, and the steepest face of a
        // level pad is 0: a point. A face with no steepness is the start of
        // the slope ramp, green (0, 255, 38), as the 3D view drew it before
        // styles - not the ramp's middle, yellow (255, 255, 38), which reads
        // as half way to the steep end.
        let surface = Arc::new(tin(
            vec![
                Point3::new(0.0, 0.0, 5.0),
                Point3::new(10.0, 0.0, 5.0),
                Point3::new(10.0, 10.0, 5.0),
                Point3::new(0.0, 10.0, 5.0),
            ],
            vec![[0, 1, 2], [0, 2, 3]],
        ));
        let mut cache = SurfacePlanCache::new();
        let style = SurfaceDisplayStyle {
            plan_faces: SurfaceFaces::Slope,
            plan_opacity: 1.0,
            ramp: String::from("slope"),
            ..bare()
        };
        let plan = katana_cad::terrain::surface_plan::surface_plan(
            &named("pad", &surface),
            &style,
            &view_of(square_box(), 0.125, *surface.bounds()),
            &mut cache,
        );
        assert_eq!(plan.image.width, 80);
        assert_eq!(pixel_at(&plan, 40, 40), opaque(0, 255, 38));
    }
}

mod surface_plan_contours {
    use super::*;

    fn contoured(interval: f64) -> SurfaceDisplayStyle {
        let mut style = bare();
        style.contours.show = true;
        style.contours.interval = interval;
        style
    }

    #[test]
    fn a_known_level_runs_where_the_surface_is_at_it_and_its_label_says_it() {
        let surface = square(false);
        let mut cache = SurfacePlanCache::new();
        let mut style = contoured(1.0);
        style.contours.major_every = 5;
        style.contours.label_major = true;
        // An eighth of a unit a pixel: contours 1 unit apart at slope 1 are
        // 8 px apart, minors drawn; the square is 80 px across.
        let mut view = view_of(boxed(-2.0, -2.0, 12.0, 12.0), 0.125, *surface.bounds());
        view.label_spacing_pixels = 1000.0; // one label a contour, half way along
        let plan = plan_of(&surface, &style, &view, &mut cache);
        assert!(plan.stats.contour_levels >= 9); // 1 to 9 at least
        assert!(!plan.stats.minor_contours_hidden);
        let mut saw_five = false;
        let mut saw_one = false;
        for contour in &plan.contours {
            if contour.level == 5.0 {
                assert!(contour.major);
                saw_five = true;
                for p in &contour.points {
                    near(p.x, 5.0, 1e-9);
                }
            }
            if contour.level == 1.0 {
                assert!(!contour.major);
                saw_one = true;
            }
        }
        assert!(saw_five);
        assert!(saw_one);
        // The level-5 contour is 10 long (80 px), over the 90 px a label
        // needs? No: 80 < 90, so only the longer majors are labelled - here
        // none.
        assert!(plan.labels.is_empty());
    }

    #[test]
    fn a_long_enough_major_is_labelled_with_its_level_along_it() {
        let surface = square(false);
        let mut cache = SurfacePlanCache::new();
        let mut style = contoured(1.0);
        style.contours.major_every = 5;
        style.contours.label_major = true;
        // A twentieth of a unit a pixel: the level-5 contour is 200 px long;
        // one label half way along it, at (5, 5), the contour running along y.
        // The majors are levels 0, 5 and 10: at 0 the whole surface is at or
        // above the level, so there is no contour (contours.rs); at 10 the
        // region at or above it is the east edge, whose rim is a contour,
        // labelled at (10, 5). Two labels.
        let mut view = view_of(boxed(-2.0, -2.0, 12.0, 12.0), 0.05, *surface.bounds());
        view.label_spacing_pixels = 1000.0;
        let plan = plan_of(&surface, &style, &view, &mut cache);
        assert_eq!(plan.labels.len(), 2);
        let five = plan
            .labels
            .iter()
            .find(|label| label.text == "5")
            .expect("a label of 5");
        near(five.at.x, 5.0, 1e-9);
        near(five.at.y, 5.0, 1e-9);
        near(five.angle.sin().abs(), 1.0, 1e-9);
        let ten = plan
            .labels
            .iter()
            .find(|label| label.text == "10")
            .expect("a label of 10");
        near(ten.at.x, 10.0, 1e-9);
    }

    #[test]
    fn minors_closer_than_three_pixels_are_left_out_and_the_majors_kept() {
        let surface = square(false);
        let mut cache = SurfacePlanCache::new();
        let mut style = contoured(1.0);
        style.contours.major_every = 5;
        // Half a unit a pixel: minors 2 px apart (hidden), majors 10 px
        // (kept).
        let plan = plan_of(
            &surface,
            &style,
            &view_of(boxed(-2.0, -2.0, 12.0, 12.0), 0.5, *surface.bounds()),
            &mut cache,
        );
        assert!(plan.stats.minor_contours_hidden);
        assert!(!plan.stats.major_contours_hidden);
        assert!(!plan.contours.is_empty());
        for contour in &plan.contours {
            assert!(contour.major);
        }
    }

    #[test]
    fn contours_are_traced_once_and_again_only_when_the_settings_change() {
        let surface = grid(20, 20, 2.0);
        let mut cache = SurfacePlanCache::new();
        let mut style = contoured(0.5);
        let view = view_of(boxed(0.0, 0.0, 40.0, 40.0), 0.1, *surface.bounds());
        let _ = plan_of(&surface, &style, &view, &mut cache);
        let _ = plan_of(&surface, &style, &view, &mut cache);
        assert_eq!(cache.contour_tracings(), 1);
        style.contours.interval = 0.25;
        let _ = plan_of(&surface, &style, &view, &mut cache);
        assert_eq!(cache.contour_tracings(), 2);
    }

    #[test]
    fn an_interval_far_too_fine_is_refused_for_the_plan_saying_why() {
        // A millionth of a unit over 10 units of relief: ten million levels
        // cross each triangle, past PLAN_MAXIMUM_CONTOUR_POINTS.
        let surface = square(false);
        let mut cache = SurfacePlanCache::new();
        let style = contoured(1.0e-6);
        let plan = plan_of(
            &surface,
            &style,
            &view_of(square_box(), 0.1, *surface.bounds()),
            &mut cache,
        );
        assert!(plan.contours.is_empty());
        assert!(plan.problem.contains("interval"), "{}", plan.problem);
    }
}

mod surface_plan_lines {
    use super::*;

    #[test]
    fn the_rim_is_one_closed_ring_and_a_breakline_is_one_line() {
        let surface = square(true);
        let mut cache = SurfacePlanCache::new();
        let style = SurfaceDisplayStyle {
            show_boundary: true,
            show_breaklines: true,
            ..bare()
        };
        let plan = plan_of(
            &surface,
            &style,
            &view_of(boxed(-1.0, -1.0, 11.0, 11.0), 0.05, *surface.bounds()),
            &mut cache,
        );
        assert_eq!(plan.boundary.len(), 1);
        // Four corners and the first again.
        assert_eq!(plan.boundary[0].len(), 5);
        assert_eq!(plan.boundary[0][0], plan.boundary[0][4]);
        assert_eq!(plan.breaklines.len(), 1);
        assert_eq!(plan.breaklines[0].len(), 2);
        let a = plan.breaklines[0][0];
        let b = plan.breaklines[0][1];
        near((a.x - b.x).abs(), 10.0, 1e-12);
        near((a.y - b.y).abs(), 10.0, 1e-12);
    }
}

mod surface_plan_points {
    use super::*;

    #[test]
    fn vertices_are_marks_when_apart_and_pixels_when_dense() {
        // Four vertices over 100 square units: 5 apart on average.
        let surface = square(false);
        let mut cache = SurfacePlanCache::new();
        let style = SurfaceDisplayStyle {
            show_points: true,
            ..bare()
        };
        let apart = plan_of(
            &surface,
            &style,
            &view_of(boxed(-1.0, -1.0, 11.0, 11.0), 1.0, *surface.bounds()),
            &mut cache,
        );
        assert_eq!(apart.stats.points_marked, 4);
        assert_eq!(apart.points.len(), 4);
        // Two units a pixel: 2.5 px apart, under 4 - pixels in the image. The
        // corners at x = 10 or y = 0 fall on the image's far edges; those on
        // it are counted.
        let dense = plan_of(
            &surface,
            &style,
            &view_of(boxed(-1.0, -1.0, 11.0, 11.0), 2.0, *surface.bounds()),
            &mut cache,
        );
        assert!(dense.points.is_empty());
        assert!(dense.stats.points_in_image >= 1);
        assert!(dense.stats.points_in_image <= 4);
    }
}

mod surface_plan_view {
    use super::*;

    #[test]
    fn a_surface_out_of_view_draws_nothing_and_an_empty_one_nothing_at_all() {
        let surface = square(false);
        let mut cache = SurfacePlanCache::new();
        let style = SurfaceDisplayStyle {
            plan_faces: SurfaceFaces::Flat,
            show_boundary: true,
            ..bare()
        };
        let away = boxed(100.0, 100.0, 120.0, 110.0);
        let plan = plan_of(
            &surface,
            &style,
            &view_of(away, 0.1, *surface.bounds()),
            &mut cache,
        );
        assert_eq!(plan.stats.triangles_in_view, 0);
        assert!(plan.image.empty());
        assert!(plan.boundary.is_empty());
        let none = katana_cad::terrain::surface_plan::surface_plan(
            &named("empty", &Arc::new(TinSurface::default())),
            &style,
            &view_of(square_box(), 0.1, square_box()),
            &mut cache,
        );
        assert_eq!(none.stats.triangles_in_view, 0);
        assert!(none.image.empty());
    }
}

mod surface_plan_image_grid {
    use super::*;

    #[test]
    fn the_grid_lines_up_with_the_views_corner_and_covers_the_surface_in_view() {
        let visible = boxed(0.0, 0.0, 100.0, 50.0);
        let bounds = boxed(10.3, 5.2, 30.7, 40.9);
        let grid = plan_image_grid(&visible, &bounds, 1.0, 1_000_000);
        // Columns floor(10.3) = 10 to ceil(30.7) = 31; rows down from the
        // top, floor(50 - 40.9) = 9 to ceil(50 - 5.2) = 45.
        assert_eq!(grid.width, 21);
        assert_eq!(grid.height, 36);
        assert_eq!(grid.r#box.min.x, 10.0);
        assert_eq!(grid.r#box.max.x, 31.0);
        assert_eq!(grid.r#box.max.y, 41.0);
        assert_eq!(grid.r#box.min.y, 5.0);
        // Apart: nothing.
        assert_eq!(
            plan_image_grid(&visible, &boxed(200.0, 0.0, 300.0, 10.0), 1.0, 100).width,
            0
        );
    }

    #[test]
    fn a_grid_over_its_cap_is_coarsened_evenly_under_it() {
        let square = boxed(0.0, 0.0, 1000.0, 1000.0);
        // 10 000 x 10 000 at a tenth: a hundred million pixels, against a cap
        // of a million.
        let grid = plan_image_grid(&square, &square, 0.1, 1_000_000);
        let pixels = grid.width as usize * grid.height as usize;
        assert!(pixels <= 1_000_000);
        assert!(pixels > 900_000);
        near(f64::from(grid.width), f64::from(grid.height), 1.0);
    }
}

mod surface_plan_cache {
    use super::*;

    #[test]
    fn more_surfaces_in_view_than_any_cap_are_kept_not_rebuilt_on_every_paint() {
        // A paint asks for every visible surface, in the same order, each
        // time. A cache of the 32 most recently used evicted, for a 33rd
        // surface, the very one the next paint asked for first: every paint
        // built every cull grid and traced every contour again (61-69 ms a
        // repaint with 33 surfaces against 3-7 ms with 32, measured in the
        // window).
        const SURFACES: usize = 33;
        // A new surface each, at its own address.
        let surfaces: Vec<Arc<TinSurface>> = (0..SURFACES).map(|_| grid(3, 3, 3.0)).collect();
        let mut cache = SurfacePlanCache::new();
        let mut style = bare();
        style.contours.show = true;
        style.contours.interval = 0.5;
        let view = view_of(boxed(0.0, 0.0, 9.0, 9.0), 0.05, *surfaces[0].bounds());
        for pass in 0..3 {
            for (i, surface) in surfaces.iter().enumerate() {
                let _ = katana_cad::terrain::surface_plan::surface_plan(
                    &named(&format!("s{i}"), surface),
                    &style,
                    &view,
                    &mut cache,
                );
            }
            // One grid and one tracing a surface, the first time only.
            assert_eq!(cache.grids_built(), 33, "after paint {}", pass + 1);
            assert_eq!(cache.contour_tracings(), 33, "after paint {}", pass + 1);
            assert_eq!(cache.surface_count(), 33);
        }
        // The caller bounds the cache: retain() lets go of all but those
        // named.
        let alive: Vec<&TinSurface> = surfaces[..10].iter().map(|surface| &**surface).collect();
        cache.retain(&alive);
        assert_eq!(cache.surface_count(), 10);
    }
}

// ---- why a style could not be drawn ------------------------------------------------------------

mod surface_problems {
    use super::*;

    fn contours_every(interval: f64) -> SurfaceContours {
        SurfaceContours {
            interval,
            ..SurfaceContours::default()
        }
    }

    #[test]
    fn contours_are_refused_exactly_when_their_crossings_pass_the_limit() {
        // The square is z = x from 0 to 10, so on each of its two triangles
        // the levels of an interval i cross floor(10 / i) times (base 0, both
        // triangles run from 0 to 10): 2 floor(10 / i) points in all, against
        // the limit of 4 000 000. i = 6e-6: 1 666 666 each, 3 333 332 points -
        // drawn. i = 4e-6: 2 500 000 each, 5 000 000 points - refused. (The
        // two sit a fifth either side of the limit, clear of the rounding of
        // 10 / i.)
        let surface = square(false);
        assert_eq!(contour_refusal(&surface, &contours_every(6.0e-6)), "");
        assert_eq!(
            contour_refusal(&surface, &contours_every(4.0e-6)),
            "contours: an interval of 0.000004 crosses this surface's triangles more than \
             4000000 times; not drawn - set a larger interval"
        );
        // The ordinary interval of the built-in Contours style is no trouble.
        assert_eq!(contour_refusal(&surface, &contours_every(1.0)), "");
    }

    #[test]
    fn an_interval_that_is_no_positive_number_is_refused_whatever_the_surface() {
        // GEOMETRIC is 1e-7: an interval at or under it is no interval. Not
        // finite is none either, nor is a base that is not finite.
        let surface = square(false);
        let expected = "contours: the interval must be a finite number greater than zero";
        for interval in [0.0, -1.0, 1.0e-8, f64::NAN, f64::INFINITY] {
            assert_eq!(
                contour_refusal(&surface, &contours_every(interval)),
                expected,
                "{interval}"
            );
        }
        let bad_base = SurfaceContours {
            interval: 1.0,
            base: f64::NAN,
            ..SurfaceContours::default()
        };
        assert_eq!(contour_refusal(&surface, &bad_base), expected);
    }

    #[test]
    fn a_level_surface_has_no_contour_to_cross_so_no_interval_is_too_fine() {
        // Every vertex at 5: no triangle spans two levels whatever the
        // interval.
        let level = tin(
            vec![
                Point3::new(0.0, 0.0, 5.0),
                Point3::new(10.0, 0.0, 5.0),
                Point3::new(10.0, 10.0, 5.0),
                Point3::new(0.0, 10.0, 5.0),
            ],
            vec![[0, 1, 2], [0, 2, 3]],
        );
        assert_eq!(contour_refusal(&level, &contours_every(1.0e-6)), "");
    }

    #[test]
    fn the_plan_and_the_check_give_one_answer() {
        // What a paint puts in SurfacePlan::problem is contour_refusal's
        // answer, word for word: one decision, said by the plan view and found
        // before a plot.
        let surface = square(false);
        let mut cache = SurfacePlanCache::new();
        let mut style = bare();
        style.contours.show = true;
        style.contours.interval = 1.0e-6;
        let plan = plan_of(
            &surface,
            &style,
            &view_of(square_box(), 0.1, *surface.bounds()),
            &mut cache,
        );
        assert!(!plan.problem.is_empty());
        assert_eq!(plan.problem, contour_refusal(&surface, &style.contours));
    }

    #[test]
    fn a_problem_reads_as_a_line_for_a_person_and_a_record_for_an_agent() {
        let problem = SurfaceProblem {
            surface: String::from("ground"),
            style: String::from("Fine"),
            reason: String::from("contours: too fine"),
        };
        assert_eq!(problem.text(), "ground (style Fine): contours: too fine");
        assert_eq!(
            problem.record(),
            "warning surface=\"ground\" style=\"Fine\" reason=\"contours: too fine\""
        );
        // A blank in a name and a quote in the reason are quoted and escaped,
        // so the record still splits into its three fields.
        let odd = SurfaceProblem {
            surface: String::from("east lot"),
            style: String::from("Two \"bands\""),
            reason: String::from("said \"no\""),
        };
        assert_eq!(
            odd.record(),
            "warning surface=\"east lot\" style=\"Two \\\"bands\\\"\" reason=\"said \\\"no\\\"\""
        );
    }

    #[test]
    fn the_styles_of_the_drawings_visible_surfaces_are_checked_without_painting_anything() {
        // Fine: contours every millionth of a unit, which the 10-unit relief
        // of the square makes 10 000 000 crossings a triangle. Three surfaces
        // use it or not: "first" and "second" in it, drawn, in the order
        // added; "hidden" in it but hidden, so not drawn; "plain" in the
        // built-in Contours (every metre), which can be traced; "outline" in
        // Standard, which shows no contours at all.
        let surface = square(false);
        let mut styles = built_in_surface_styles();
        let mut fine = bare();
        fine.name = String::from("Fine");
        fine.contours.show = true;
        fine.contours.interval = 1.0e-6;
        styles.styles.push(fine.clone());
        let mut store = SurfaceStore::default();
        for (name, style, visible) in [
            ("first", "Fine", true),
            ("hidden", "Fine", false),
            ("plain", "Contours", true),
            ("outline", "Standard", true),
            ("second", "Fine", true),
        ] {
            crate::ok(store.add(NamedSurface {
                name: String::from(name),
                surface: Some(Arc::clone(&surface)),
                source: String::from("test"),
                style: String::from(style),
                visible,
            }));
        }

        let problems = surface_style_problems(&store, &styles);
        assert_eq!(problems.len(), 2);
        let reason = contour_refusal(&surface, &fine.contours);
        assert!(!reason.is_empty());
        assert_eq!(
            problems[0].problem,
            SurfaceProblem {
                surface: String::from("first"),
                style: String::from("Fine"),
                reason: reason.clone(),
            }
        );
        assert_eq!(
            problems[1].problem,
            SurfaceProblem {
                surface: String::from("second"),
                style: String::from("Fine"),
                reason: reason.clone(),
            }
        );
        assert_eq!(problems[0].bounds.min.x, 0.0);
        assert_eq!(problems[0].bounds.max.x, 10.0);
        assert_eq!(problems[0].bounds.max.y, 10.0);

        // No surfaces, no problems; the surface mended, none.
        assert!(surface_style_problems(&SurfaceStore::default(), &styles).is_empty());
        if let Some(last) = styles.styles.last_mut() {
            last.contours.interval = 1.0;
        }
        assert!(surface_style_problems(&store, &styles).is_empty());
    }
}
