// Zooming a 3D view towards what it draws (scene_zoom.rs; docs/render.md,
// "Zooming towards the cursor"; tests/cad/test_scene_zoom.cpp).
//
// The pick's expected values are worked by hand from scenes chosen so that
// the arithmetic is short: a camera looking due north from (0, -300, 5) at a
// block and the ground, where every depth is a difference of northings. The
// zoom's are properties of the zoom's definition - a notch magnifies what is
// under the cursor by 1.15, so its depth divides by 1.15 - measured against
// the plane the cursor's ray is met with here, never against what the zoom
// says it picked. The limits are the two a view cannot draw past: the near
// plane's floor, tolerance::GEOMETRIC / Camera::NEAR_PIVOT_FLOOR, and the
// bound of the double arithmetic, 16 x 2^-53 of the point's distance from the
// origin (Higham's bound for dot products; scene_zoom.rs works the count).

use crate::{double_eq, near, ok, tin};
use katana_cad::document::Document;
use katana_cad::scene::{SceneBuilder, SceneLayers, SceneOptions, SceneSurface, scene_depth_box};
use katana_cad::scene_zoom::{
    PickOptions, ScenePick, ScenePickSource, minimum_approach, pick_drawn_point, zoom_anchor,
    zoom_at_pixel,
};
use katana_commands::create_entities;
use katana_entity::entity::set_heights;
use katana_entity::{Entity, Geometry, PointGeometry};
use katana_geometry::{Point2, Point3, Polyline2};
use katana_math::Vec3;
use katana_render::{Camera, DrawList, Projection, Rgba, StandardView, VertexIndex, rgba};
use katana_terrain::TinSurface;

const INK: Rgba = rgba(200, 200, 200);

/// The rectangle [x0, x1] x [y0, y1] at height z, two triangles.
fn add_rectangle(list: &mut DrawList, x0: f64, y0: f64, x1: f64, y1: f64, z: f64) {
    let a = list.add_vertex(Vec3::new(x0, y0, z), INK);
    let b = list.add_vertex(Vec3::new(x1, y0, z), INK);
    let c = list.add_vertex(Vec3::new(x1, y1, z), INK);
    let d = list.add_vertex(Vec3::new(x0, y1, z), INK);
    list.add_triangle(a, b, c);
    list.add_triangle(a, c, d);
}

/// A closed box, twelve triangles.
fn add_box(list: &mut DrawList, lo: Vec3, hi: Vec3) {
    let mut corner: [VertexIndex; 8] = [0; 8];
    for (k, index) in corner.iter_mut().enumerate() {
        *index = list.add_vertex(
            Vec3::new(
                if (k & 1) != 0 { hi.x } else { lo.x },
                if (k & 2) != 0 { hi.y } else { lo.y },
                if (k & 4) != 0 { hi.z } else { lo.z },
            ),
            INK,
        );
    }
    let faces: [[usize; 4]; 6] = [
        [0, 1, 3, 2],
        [4, 6, 7, 5],
        [0, 4, 5, 1],
        [2, 3, 7, 6],
        [0, 2, 6, 4],
        [1, 5, 7, 3],
    ];
    for f in faces {
        list.add_triangle(corner[f[0]], corner[f[1]], corner[f[2]]);
        list.add_triangle(corner[f[0]], corner[f[2]], corner[f[3]]);
    }
}

/// The scene the pick is worked on: ground 200 m square at z = 0, a block
/// 10 m square and 10 m high in its middle, a line in the air in front of the
/// block, a line behind it at the eye's height, a point, and the grid's
/// square 260 m across on the datum. Laid out as SceneBuilder lays out a
/// scene: the ground and the block in the terrain, the rest in the drawing.
fn pick_scene() -> SceneLayers {
    let mut layers = SceneLayers::default();
    add_rectangle(&mut layers.terrain, -100.0, -100.0, 100.0, 100.0, 0.0);
    add_box(
        &mut layers.terrain,
        Vec3::new(-5.0, -5.0, 0.0),
        Vec3::new(5.0, 5.0, 10.0),
    );
    layers.entities.add_segment(
        Vec3::new(-30.0, -50.0, 1.0),
        Vec3::new(30.0, -50.0, 1.0),
        INK,
        1.0,
        1.5,
    );
    layers.entities.add_segment(
        Vec3::new(-30.0, 20.0, 5.0),
        Vec3::new(30.0, 20.0, 5.0),
        INK,
        1.0,
        1.5,
    );
    let point = layers.entities.add_vertex(Vec3::new(10.0, -30.0, 7.0), INK);
    layers.entities.add_point(point, 5.0, 1.5);
    let corners = [
        Vec3::new(-130.0, -130.0, 0.0),
        Vec3::new(130.0, -130.0, 0.0),
        Vec3::new(130.0, 130.0, 0.0),
        Vec3::new(-130.0, 130.0, 0.0),
    ];
    for k in 0..4 {
        layers
            .grid
            .add_segment(corners[k], corners[(k + 1) % 4], INK, 1.0, 0.0);
    }
    layers.datum = 0.0;
    layers.bounds = layers.terrain.bounds();
    layers.bounds.expand(layers.entities.bounds());
    layers
}

/// Looking due north from (0, -300, 5): the depth of a point is its northing
/// plus 300.
fn north_camera() -> Camera {
    let mut camera = Camera::default();
    camera.set_viewport_size(400, 300);
    camera.set_standard_view(StandardView::Front);
    camera.set_target(Vec3::new(0.0, 0.0, 5.0));
    camera.set_distance(300.0);
    camera
}

/// The pixel whose ray passes through `world`: ray_through_pixel(x, y) goes
/// through the pixel's centre, (x + 0.5, y + 0.5) on the screen.
fn pixel_of(camera: &Camera, world: Vec3) -> (f64, f64) {
    let screen = camera.project(world).expect("the point is in front");
    (screen.x - 0.5, screen.y - 0.5)
}

#[track_caller]
fn expect_pick(pick: Option<ScenePick>, source: ScenePickSource, depth: f64, point: Vec3) {
    let pick = pick.expect("a pick");
    assert_eq!(pick.source, source);
    near(pick.depth, depth, 1e-9 * depth);
    near((pick.point - point).length(), 0.0, 1e-9 * depth);
}

fn pick(layers: &SceneLayers, camera: &Camera, x: f64, y: f64) -> Option<ScenePick> {
    pick_drawn_point(layers, camera, x, y, &PickOptions::default())
}

// ---- the pick ------------------------------------------------------------------------------

/// Rolling ground 400 m square in 40 x 40 cells, 3D strings in the air over
/// it, survey points at their own heights and plan linework draped on it,
/// built by SceneBuilder as the 3D view builds it.
struct Survey {
    layers: SceneLayers,
}

impl Survey {
    fn new() -> Self {
        let height = |x: f64, y: f64| 20.0 + 6.0 * (x / 57.0).sin() * (y / 83.0).cos();
        let mut vertices = Vec::new();
        for j in 0..=40 {
            for i in 0..=40 {
                let x = 10.0 * f64::from(i);
                let y = 10.0 * f64::from(j);
                vertices.push(Point3::new(x, y, height(x, y)));
            }
        }
        let mut triangles = Vec::new();
        for j in 0..40u32 {
            for i in 0..40u32 {
                let v0 = j * 41 + i;
                triangles.push([v0, v0 + 1, v0 + 42]);
                triangles.push([v0, v0 + 42, v0 + 41]);
            }
        }
        let surface: TinSurface = tin(vertices, triangles);
        let mut entities = Vec::new();
        for k in 0..8 {
            let k = f64::from(k);
            let path: Vec<Point2> = (0..=10)
                .map(|v| Point2::new(20.0 + 36.0 * f64::from(v), 30.0 + 45.0 * k))
                .collect();
            let heights = vec![Some(40.0 + k); path.len()];
            let mut string = Entity {
                geometry: Geometry::Polyline(Polyline2::new(path, false)),
                ..Entity::default()
            };
            set_heights(&mut string.properties, &heights);
            entities.push(string);

            let mut point = Entity {
                geometry: Geometry::Point(PointGeometry {
                    position: Point2::new(50.0 + 40.0 * k, 370.0 - 40.0 * k),
                }),
                ..Entity::default()
            };
            set_heights(&mut point.properties, &[Some(35.0)]);
            entities.push(point);

            entities.push(Entity {
                geometry: Geometry::Polyline(Polyline2::new(
                    vec![
                        Point2::new(15.0 + 45.0 * k, 10.0),
                        Point2::new(15.0 + 45.0 * k, 390.0),
                    ],
                    false,
                )),
                ..Entity::default()
            });
        }
        let mut document = Document::default();
        ok(document.execute(create_entities(entities)));

        let surfaces = [SceneSurface {
            name: String::from("Ground"),
            surface: Some(&surface),
            ..SceneSurface::default()
        }];
        let options = SceneOptions::default();
        let mut builder = SceneBuilder::new();
        let mut layers = SceneLayers::default();
        builder.build_terrain(&surfaces, &[], &options, &mut layers);
        builder.build_entities(&document, &surfaces, &options, &mut layers);
        builder.build_selection(&document, &surfaces, &options, &mut layers);
        builder.build_grid(&options, &mut layers);
        Self { layers }
    }
}

mod scene_pick {
    use super::*;

    #[test]
    fn the_nearest_triangle_under_the_pixel_is_found_and_a_line_behind_it_is_not() {
        // The middle pixel's ray runs due north at z = 5. It meets the block's
        // south face (y = -5) 295 m from the eye, at (0, -5, 5) - exactly on
        // the diagonal the face is cut along, which both of its triangles
        // count - before the line behind the block, which lies on the same ray
        // at y = 20, 320 m away.
        let layers = pick_scene();
        let camera = north_camera();
        expect_pick(
            pick(&layers, &camera, 199.5, 149.5),
            ScenePickSource::Terrain,
            295.0,
            Vec3::new(0.0, -5.0, 5.0),
        );

        // The ground 60 m south of the middle: 240 m away, with nothing before
        // it.
        let (gx, gy) = pixel_of(&camera, Vec3::new(0.0, -60.0, 0.0));
        expect_pick(
            pick(&layers, &camera, gx, gy),
            ScenePickSource::Terrain,
            240.0,
            Vec3::new(0.0, -60.0, 0.0),
        );
    }

    #[test]
    fn a_line_or_a_point_within_the_aperture_is_found_in_front_of_what_is_behind_it() {
        // The ray through (0, -50, 1), on the line in the air, goes on down to
        // the block's south face 295 m away, 4.72 m lower; the line, 250 m
        // away, is nearer. Four pixels above it the line is
        // 4 x 250 x 2 tan(22.5 deg) / 300 = 2.76 m below the ray, outside the
        // 3 px aperture (2.07 m there), and the ray goes on to the block's
        // face.
        let layers = pick_scene();
        let camera = north_camera();
        let (lx, ly) = pixel_of(&camera, Vec3::new(0.0, -50.0, 1.0));
        expect_pick(
            pick(&layers, &camera, lx, ly),
            ScenePickSource::Drawing,
            250.0,
            Vec3::new(0.0, -50.0, 1.0),
        );
        let above = pick(&layers, &camera, lx, ly - 4.0).expect("a pick above");
        assert_eq!(above.source, ScenePickSource::Terrain);
        near(above.depth, 295.0, 1e-9 * 295.0);
        near(above.point.y, -5.0, 1e-9);
        // Two pixels above it, inside the aperture, the line is still what is
        // there: its point nearest the ray, straight below it. (Two pixels
        // BELOW it the ray meets the ground 232 m away, in front of the line,
        // and the ground is what the pixel shows.)
        expect_pick(
            pick(&layers, &camera, lx, ly - 2.0),
            ScenePickSource::Drawing,
            250.0,
            Vec3::new(0.0, -50.0, 1.0),
        );
        let below = pick(&layers, &camera, lx, ly + 2.0).expect("a pick below");
        assert_eq!(below.source, ScenePickSource::Terrain);
        assert!(below.depth < 250.0);
        // With no aperture, the line must be exactly under the pixel's centre.
        let exact = PickOptions {
            aperture: 0.0,
            ..PickOptions::default()
        };
        let missed =
            pick_drawn_point(&layers, &camera, lx, ly - 2.0, &exact).expect("a pick, missed");
        assert_eq!(missed.source, ScenePickSource::Terrain);
        near(missed.depth, 295.0, 1e-9 * 295.0);

        // The point at (10, -30, 7): 270 m away, the block behind it.
        let (px, py) = pixel_of(&camera, Vec3::new(10.0, -30.0, 7.0));
        expect_pick(
            pick(&layers, &camera, px, py),
            ScenePickSource::Drawing,
            270.0,
            Vec3::new(10.0, -30.0, 7.0),
        );
    }

    #[test]
    fn with_nothing_drawn_there_the_pick_is_the_datums_plane_inside_the_scenes_box_only() {
        // (0, -115, 0) is off the ground (which ends at y = -100) and inside
        // the grid's square (which reaches y = -130): the datum's plane, 185 m
        // away. (0, -140, 0) is past the grid: nothing, rather than a pivot
        // out where nothing is drawn.
        let layers = pick_scene();
        let camera = north_camera();
        let (dx, dy) = pixel_of(&camera, Vec3::new(0.0, -115.0, 0.0));
        expect_pick(
            pick(&layers, &camera, dx, dy),
            ScenePickSource::Datum,
            185.0,
            Vec3::new(0.0, -115.0, 0.0),
        );
        let (ox, oy) = pixel_of(&camera, Vec3::new(0.0, -140.0, 0.0));
        assert!(pick(&layers, &camera, ox, oy).is_none());
        // A ray rising into the sky meets neither the scene nor the datum.
        assert!(pick(&layers, &camera, 199.5, 20.0).is_none());
    }

    #[test]
    fn a_ray_parallel_to_the_datum_an_empty_scene_and_an_unusable_pixel_pick_nothing() {
        let layers = pick_scene();
        let camera = north_camera();
        // The middle row runs level at z = 5; at the right edge it passes the
        // block by 160 m and never comes down to the datum.
        assert!(pick(&layers, &camera, 399.0, 149.5).is_none());
        // No scene at all: no box, so not even the datum.
        assert!(pick(&SceneLayers::default(), &camera, 199.5, 149.5).is_none());
        // A pixel that is not a number, and a camera with no viewport.
        assert!(pick(&layers, &camera, f64::NAN, 149.5).is_none());
        assert!(pick(&layers, &camera, 199.5, f64::INFINITY).is_none());
        let mut unsized_camera = camera;
        unsized_camera.set_viewport_size(0, 0);
        assert!(pick(&layers, &unsized_camera, 199.5, 149.5).is_none());
    }

    #[test]
    fn the_culled_pick_finds_exactly_what_the_full_one_does() {
        // The cull skips a primitive whose vertices all lie beyond one side of
        // the ray (a triangle) or of the aperture (a line or a point), from
        // one pass over the vertices. It must change no answer: every 6th
        // pixel of two views of the survey, each projection, picked both ways
        // - and the pixels must reach every kind of answer, or they prove
        // nothing about it.
        let survey = Survey::new();
        assert!(!survey.layers.terrain.triangles.is_empty());
        assert!(!survey.layers.entities.lines.is_empty());
        assert!(!survey.layers.entities.points.is_empty());
        let full = PickOptions {
            cull: false,
            ..PickOptions::default()
        };
        let mut terrain = 0;
        let mut drawing = 0;
        let mut datum = 0;
        let mut nothing = 0;
        for projection in [Projection::Perspective, Projection::Orthographic] {
            for elevation in [0.61, 0.2] {
                let mut camera = Camera::default();
                camera.set_viewport_size(480, 320);
                camera.set_projection(projection);
                camera.set_standard_view(StandardView::IsoSouthWest);
                camera.set_orientation(camera.azimuth(), elevation);
                assert!(camera.frame_with_margin(survey.layers.bounds, 0.3));
                for y in (0..320).step_by(6) {
                    for x in (0..480).step_by(6) {
                        let (x, y) = (f64::from(x), f64::from(y));
                        let culled = pick(&survey.layers, &camera, x, y);
                        let every = pick_drawn_point(&survey.layers, &camera, x, y, &full);
                        assert_eq!(culled.is_some(), every.is_some(), "{x},{y}");
                        let (Some(culled), Some(every)) = (culled, every) else {
                            nothing += 1;
                            continue;
                        };
                        assert_eq!(culled.depth, every.depth, "{x},{y}");
                        assert_eq!(culled.point.x, every.point.x, "{x},{y}");
                        assert_eq!(culled.point.y, every.point.y, "{x},{y}");
                        assert_eq!(culled.point.z, every.point.z, "{x},{y}");
                        assert_eq!(culled.source, every.source, "{x},{y}");
                        match culled.source {
                            ScenePickSource::Terrain => terrain += 1,
                            ScenePickSource::Drawing => drawing += 1,
                            ScenePickSource::Datum => datum += 1,
                            ScenePickSource::Reach => {}
                        }
                    }
                }
            }
        }
        assert!(terrain > 1000, "terrain {terrain}");
        assert!(drawing > 100, "drawing {drawing}");
        assert!(datum > 100, "datum {datum}");
        assert!(nothing > 100, "nothing {nothing}");
    }
}

// ---- the zoom ----------------------------------------------------------------------------

const PLANE: f64 = -10.0;
const PER_NOTCH: f64 = 1.15;

/// Ground 200 m square at z = -10 under a camera framed as the audit framed
/// it: target (0, 0, 0) 100 m from the eye, from the south-west and 35 deg
/// up, 1200 x 800. The target is 10 m above the ground, so the ground above
/// the middle of the view lies beyond the target's plane and the ground below
/// it nearer.
fn plane_scene() -> SceneLayers {
    let mut layers = SceneLayers::default();
    add_rectangle(&mut layers.terrain, -100.0, -100.0, 100.0, 100.0, PLANE);
    layers.datum = PLANE;
    layers.bounds = layers.terrain.bounds();
    layers
}

fn plane_camera(projection: Projection) -> Camera {
    let mut camera = Camera::default();
    camera.set_viewport_size(1200, 800);
    camera.set_projection(projection);
    camera.set_standard_view(StandardView::IsoSouthWest);
    camera.set_target(Vec3::new(0.0, 0.0, 0.0));
    camera.set_distance(100.0);
    camera.set_orthographic_height(100.0);
    camera
}

/// Where the ray through pixel (x, y) meets the plane z = height: by hand,
/// not by the pick.
fn plane_under_at(camera: &Camera, x: f64, y: f64, height: f64) -> Option<Vec3> {
    let ray = camera.ray_through_pixel(x, y);
    if ray.direction.z == 0.0 {
        return None;
    }
    let t = (height - ray.origin.z) / ray.direction.z;
    (t > 0.0).then_some(ray.at(t))
}

fn plane_under(camera: &Camera, x: f64, y: f64) -> Option<Vec3> {
    plane_under_at(camera, x, y, PLANE)
}

fn depth_of(camera: &Camera, point: Vec3) -> f64 {
    (point - camera.eye()).dot(camera.forward())
}

/// A model in the middle of an empty view, as a docked view opens: ground
/// 40 m square at z = -10, the grid's square 52 m across on the datum round
/// it, seen from the south-west 150 m from (0, 0, -10). The eye is at
/// (-86.6, -86.6, 76.6); the box reaches 180 m deep, to the grid's far corner
/// (26, 26, -10): 2 x 112.6 x 0.5774 + 86.6 x 0.5774. The bottom-left corner
/// of the view meets the datum at (-89.0, -6.3, -10), off the grid and 95.0 m
/// deep; the top of the view meets it at (178, 178), 355 m deep, beyond what
/// the scene reaches.
fn island_scene() -> SceneLayers {
    let mut layers = SceneLayers::default();
    add_rectangle(&mut layers.terrain, -20.0, -20.0, 20.0, 20.0, PLANE);
    let corners = [
        Vec3::new(-26.0, -26.0, PLANE),
        Vec3::new(26.0, -26.0, PLANE),
        Vec3::new(26.0, 26.0, PLANE),
        Vec3::new(-26.0, 26.0, PLANE),
    ];
    for k in 0..4 {
        layers
            .grid
            .add_segment(corners[k], corners[(k + 1) % 4], INK, 1.0, 0.0);
    }
    layers.datum = PLANE;
    layers.bounds = layers.terrain.bounds();
    layers
}

fn island_camera() -> Camera {
    let mut camera = Camera::default();
    camera.set_viewport_size(1200, 800);
    camera.set_standard_view(StandardView::IsoSouthWest);
    camera.set_target(Vec3::new(0.0, 0.0, PLANE));
    camera.set_distance(150.0);
    camera
}

/// 2^-53, the unit roundoff of a double.
const UNIT_ROUNDOFF: f64 = f64::EPSILON / 2.0;

mod scene_zoom {
    use super::*;

    #[test]
    fn every_notch_magnifies_what_is_drawn_under_the_cursor_by_the_notch_factor() {
        // Above the middle, the ground lies beyond the target's plane:
        // anchored on that plane, the sample terrain's ground was magnified
        // 4.8x in 30 notches and 5% more in the next ten (docs/render.md).
        // Anchored on the ground, the depth of the ground under the cursor
        // divides by exactly 1.15 a notch, and the same point of the ground
        // stays under the cursor's pixel.
        let layers = plane_scene();
        let mut camera = plane_camera(Projection::Perspective);
        let (px, py) = (700.0, 150.0);
        let start = plane_under(&camera, px, py).expect("the ground under the cursor");
        let first = depth_of(&camera, start);
        assert!(
            first > 1.2 * camera.distance(),
            "not beyond the target's plane: proves nothing"
        );
        let mut depth = first;
        for notch in 1..=50 {
            let result = zoom_at_pixel(&layers, &mut camera, 1.0, px, py);
            let anchor = result.anchor.expect("an anchor");
            assert_eq!(anchor.source, ScenePickSource::Terrain);
            assert!(!result.limited);
            let under = plane_under(&camera, px, py).expect("the ground under the cursor");
            let now = depth_of(&camera, under);
            assert!(
                (depth / now - PER_NOTCH).abs() <= 1e-9,
                "notch {notch}: {}",
                depth / now
            );
            depth = now;
            let screen = camera.project(start).expect("the start in front");
            assert!((screen.x - (px + 0.5)).abs() <= 1e-6, "notch {notch}");
            assert!((screen.y - (py + 0.5)).abs() <= 1e-6, "notch {notch}");
        }
        near(
            first / depth,
            PER_NOTCH.powf(50.0),
            1e-6 * PER_NOTCH.powf(50.0),
        );
    }

    #[test]
    fn zooming_in_never_takes_the_eye_through_what_is_under_the_cursor() {
        // Below the middle the ground is nearer than the target's plane, and
        // the old anchor was under it: the eye went through the ground at
        // notch -ln(1 - depth / distance) / ln 1.15. Anchored on the ground,
        // the eye closes 13% of the way each notch and stays above it.
        let layers = plane_scene();
        let mut camera = plane_camera(Projection::Perspective);
        let (px, py) = (500.0, 780.0);
        let start = plane_under(&camera, px, py).expect("the ground under the cursor");
        let first = depth_of(&camera, start);
        assert!(
            first < 0.8 * camera.distance(),
            "not nearer than the target's plane"
        );
        let crossing = -(1.0 - first / camera.distance()).ln() / PER_NOTCH.ln();
        assert!(
            crossing < 20.0,
            "the old anchor would not have gone through: proves nothing"
        );
        for notch in 1..=40 {
            let _ = zoom_at_pixel(&layers, &mut camera, 1.0, px, py);
            assert!(
                camera.eye().z > PLANE,
                "the eye is under the ground at notch {notch}"
            );
            let under = plane_under(&camera, px, py).expect("the ground under the cursor");
            let expected = first * PER_NOTCH.powf(-f64::from(notch));
            near(depth_of(&camera, under), expected, 1e-9 * expected);
        }
    }

    #[test]
    fn zooming_in_stops_a_tenth_of_a_millimetre_short_and_a_notch_out_still_shrinks_by_the_factor()
    {
        // The near plane stays a thousandth of the pivot's distance in front
        // of the eye (Camera::NEAR_PIVOT_FLOOR) and never nearer than
        // tolerance::GEOMETRIC, 1e-7: so the pivot, what the zoom went
        // towards, comes no nearer than 1e-7 / 1e-3 = 1e-4. The double
        // arithmetic's own bound is far below that here: 16 x 2^-53 x 110 m
        // (the point's distance from the origin) over a pixel's angle,
        // 2 tan(22.5 deg) / 800, and a tenth of a pixel is 2e-9 m. The zoom
        // stops there, the point still under the cursor, the near plane a
        // thousand times nearer than it.
        let layers = plane_scene();
        let mut camera = plane_camera(Projection::Perspective);
        let (px, py) = (700.0, 150.0);
        let start = plane_under(&camera, px, py).expect("the ground under the cursor");
        double_eq(minimum_approach(&camera, start), 1e-4);
        // From about 150 m, 1.15^n = 1.5e6 at n = 102.
        let mut limited = false;
        for _ in 0..130 {
            limited = zoom_at_pixel(&layers, &mut camera, 1.0, px, py).limited || limited;
        }
        assert!(limited);
        let under = plane_under(&camera, px, py).expect("the ground under the cursor");
        let depth = depth_of(&camera, under);
        near(depth, 1e-4, 1e-9 * 1e-4);
        near((under - start).length(), 0.0, 1e-6);
        // Held there: another notch in moves nothing. The notch picks the
        // ground again, and its depth is a difference of coordinates some
        // 100 m from the origin (a unit in the last place of 1.4e-14 m) over
        // the 0.1 mm between eye and ground: good to a few parts in 1e10, so
        // the factor that holds the eye at the limit is 1 to that, and the eye
        // moves by less than a unit in the last place of its coordinates.
        let eye = camera.eye();
        let held = zoom_at_pixel(&layers, &mut camera, 1.0, px, py);
        assert!(held.limited);
        near(held.factor, 1.0, 1e-8);
        near((camera.eye() - eye).length(), 0.0, 1e-12);
        // What the view draws through it keeps the ground well in front of
        // the near plane: the box reaches behind the eye, so the floor
        // decides, and the pivot's share of it, 1e-4 x 1e-3, is far below the
        // far plane's.
        let mut drawn = camera;
        assert!(drawn.fit_depth_range(scene_depth_box(&layers)));
        near(drawn.near_plane(), 1e-7, 1e-15);
        assert!(depth / drawn.near_plane() > 999.0);
        // And a notch out undoes a notch in.
        let _ = zoom_at_pixel(&layers, &mut camera, -1.0, px, py);
        let out = plane_under(&camera, px, py).expect("the ground under the cursor");
        near(depth_of(&camera, out) / depth, PER_NOTCH, 1e-8);
    }

    #[test]
    fn an_enormous_notch_count_stops_at_the_minimum_approach_in_one_step() {
        // 1.15^-1e6 underflows to zero, which the limit catches; 1.15^1e6 out
        // overflows, which is refused rather than sending the eye to infinity.
        let layers = plane_scene();
        let mut camera = plane_camera(Projection::Perspective);
        let into = zoom_at_pixel(&layers, &mut camera, 1.0e6, 700.0, 150.0);
        assert!(into.limited);
        let under = plane_under(&camera, 700.0, 150.0).expect("the ground under the cursor");
        near(depth_of(&camera, under), 1e-4, 1e-12);
        let eye = camera.eye();
        let out = zoom_at_pixel(&layers, &mut camera, -1.0e6, 700.0, 150.0);
        assert_eq!(out.factor, 1.0);
        assert!(camera.distance().is_finite());
        near((camera.eye() - eye).length(), 0.0, 1e-9);
    }

    #[test]
    fn an_orthographic_zoom_magnifies_by_the_factor_and_stops_where_a_perspective_one_would() {
        // Every point of an orthographic pixel's ray stays under it, so there
        // is no depth to pick and nothing ever stalled: a notch divides the
        // height by 1.15. It stops at the height a perspective view shows at
        // the minimum approach, 2 x 1e-4 x tan(22.5 deg) = 8.2843e-5 m, so P
        // keeps the limit where it was.
        let layers = plane_scene();
        let mut camera = plane_camera(Projection::Orthographic);
        let mut height = camera.orthographic_height();
        for notch in 1..=20 {
            let result = zoom_at_pixel(&layers, &mut camera, 1.0, 700.0, 150.0);
            assert!(result.anchor.is_none());
            assert!(
                (height / camera.orthographic_height() - PER_NOTCH).abs() <= 1e-12,
                "{notch}"
            );
            height = camera.orthographic_height();
        }
        // From 100 m / 1.15^20, 1.15^n = 7.4e4 at n = 80.
        for _ in 0..100 {
            let _ = zoom_at_pixel(&layers, &mut camera, 1.0, 700.0, 150.0);
        }
        near(
            camera.orthographic_height(),
            2.0 * 1e-4 * 0.41421356237309503,
            1e-15,
        );
    }

    #[test]
    fn an_orthographic_view_of_a_kilometres_wide_scene_zooms_on_past_where_the_scenes_size_stopped_it()
     {
        // The elevation view of a 12 km ground framed 8480 m tall: 120 notches
        // in divide that by 1.15^120 = 1.9e7, to 0.44 mm, and nothing about
        // the scene's size stops it. A limit of a fraction of the scene's box
        // - 1e-4 of its 17 km diagonal, 1.7 m of view at a 45 degree field -
        // held it at 1.87 m, and the elevation view had never stalled.
        let mut layers = SceneLayers::default();
        add_rectangle(&mut layers.terrain, 0.0, 0.0, 12000.0, 12000.0, 90.0);
        layers.datum = 90.0;
        layers.bounds = layers.terrain.bounds();
        let mut camera = Camera::default();
        camera.set_viewport_size(1200, 800);
        camera.set_projection(Projection::Orthographic);
        camera.set_standard_view(StandardView::Front);
        camera.set_target(Vec3::new(6000.0, 6000.0, 90.0));
        camera.set_distance(16000.0);
        camera.set_orthographic_height(8480.0);
        for notch in 0..120 {
            assert!(
                !zoom_at_pixel(&layers, &mut camera, 1.0, 600.0, 400.0).limited,
                "{notch}"
            );
        }
        let expected = 8480.0 / PER_NOTCH.powf(120.0);
        near(camera.orthographic_height(), expected, 1e-9 * expected);
    }

    #[test]
    fn off_the_model_a_notch_zooms_towards_the_datum_and_the_eye_never_goes_under_it() {
        // Nothing is drawn under the bottom-left corner, and the datum there
        // is outside the grid. The target's plane, 150 m deep, lies beyond the
        // datum's point, 95.0 m deep, so the old anchor was under the ground
        // and the eye went through it at -ln(1 - 95.0 / 150) / ln 1.15 = 7.2
        // notches: the whole view blank from then on. Anchored on the datum,
        // its point under the cursor divides its depth by 1.15 a notch and the
        // eye stays above it.
        let layers = island_scene();
        let mut camera = island_camera();
        let (px, py) = (5.0, 795.0);
        assert!(
            pick(&layers, &camera, px, py).is_none(),
            "something is drawn under the cursor, so this proves nothing"
        );
        let start = plane_under(&camera, px, py).expect("the datum under the cursor");
        let first = depth_of(&camera, start);
        assert!(
            first < 0.7 * camera.distance(),
            "the datum is not nearer than the target's plane"
        );
        let mut depth = first;
        for notch in 1..=60 {
            let result = zoom_at_pixel(&layers, &mut camera, 1.0, px, py);
            let anchor = result.anchor.expect("an anchor");
            assert_eq!(anchor.source, ScenePickSource::Datum, "{notch}");
            assert!(
                camera.eye().z > PLANE,
                "the eye is under the datum at notch {notch}"
            );
            let under = plane_under(&camera, px, py).expect("the datum under the cursor");
            let now = depth_of(&camera, under);
            assert!(
                (depth / now - PER_NOTCH).abs() <= 1e-9,
                "notch {notch}: {}",
                depth / now
            );
            depth = now;
            near((under - start).length(), 0.0, 1e-9 * first);
        }
    }

    #[test]
    fn beyond_the_scenes_reach_a_notch_zooms_as_deep_as_it_reaches_so_nothing_drawn_stalls() {
        // The top of the view meets the datum 355 m deep, twice as deep as
        // the scene reaches (180 m). The target's plane, 150 m deep, left the
        // grid's far corner beyond the anchor, so its magnification tended to
        // 180 / (180 - 150) = 6 and stopped; anchored on the datum's point, a
        // notch would cover 13% of 355 m and put the model behind the eye in
        // four. Anchored as deep as the scene reaches, the far corner divides
        // its depth by exactly 1.15 a notch, and whatever is drawn, all of it
        // nearer, divides its own by at least that until it leaves the view.
        let layers = island_scene();
        let mut camera = island_camera();
        let (px, py) = (600.0, 5.0);
        let datum = plane_under(&camera, px, py).expect("the datum under the cursor");
        let far = Vec3::new(26.0, 26.0, PLANE);
        let ground = Vec3::new(20.0, 20.0, PLANE); // the model's far corner
        assert!(
            depth_of(&camera, datum) > 1.5 * depth_of(&camera, far),
            "the datum is within the scene's reach, so this proves nothing"
        );
        let mut reach = depth_of(&camera, far);
        near(reach, 2.0 * 112.6 * 0.5774 + 86.6 * 0.5774, 0.1);
        let mut model = depth_of(&camera, ground);
        for notch in 1..=40 {
            let result = zoom_at_pixel(&layers, &mut camera, 1.0, px, py);
            let anchor = result.anchor.expect("an anchor");
            assert_eq!(anchor.source, ScenePickSource::Reach, "{notch}");
            assert!(camera.eye().z > PLANE, "{notch}");
            let now = depth_of(&camera, far);
            assert!(
                (reach / now - PER_NOTCH).abs() <= 1e-9,
                "notch {notch}: {}",
                reach / now
            );
            reach = now;
            let model_now = depth_of(&camera, ground);
            if model_now > 0.0 {
                assert!(model / model_now >= PER_NOTCH - 1e-9, "notch {notch}");
            }
            model = model_now;
        }
    }

    #[test]
    fn a_level_ray_out_of_the_scene_takes_the_depth_the_scene_reaches() {
        // Nearly level, the top of the view is sky: the ray never comes down
        // to the datum, so the zoom goes as deep as the scene reaches, its
        // box's furthest corner along the view.
        let layers = plane_scene();
        let mut camera = plane_camera(Projection::Perspective);
        camera.set_orientation(camera.azimuth(), 0.05);
        assert!(
            pick(&layers, &camera, 600.0, 20.0).is_none(),
            "the pixel is not sky, so this proves nothing"
        );
        assert!(
            camera.ray_through_pixel(600.0, 20.0).direction.z > 0.0,
            "not a rising ray"
        );
        let mut reach: f64 = 0.0;
        for x in [-100.0, 100.0] {
            for y in [-100.0, 100.0] {
                reach = katana_math::cpp::max(reach, depth_of(&camera, Vec3::new(x, y, PLANE)));
            }
        }
        let anchor = zoom_anchor(&layers, &camera, 600.0, 20.0).expect("an anchor");
        assert_eq!(anchor.source, ScenePickSource::Reach);
        near(anchor.depth, reach, 1e-9 * reach);
        near(depth_of(&camera, anchor.point), reach, 1e-9 * reach);
    }

    #[test]
    fn with_the_whole_scene_behind_the_eye_a_notch_zooms_about_the_targets_plane_as_it_always_did()
    {
        // Turned round to face away from the scene there is nothing in front
        // to go towards: the zoom is Camera::dolly_at_pixel's, to the bit.
        let layers = plane_scene();
        let mut camera = plane_camera(Projection::Perspective);
        camera.set_target(Vec3::new(-300.0, -300.0, 50.0));
        camera.set_orientation(camera.azimuth() + 3.141592653589793, 0.2);
        assert!(
            zoom_anchor(&layers, &camera, 600.0, 400.0).is_none(),
            "something is in front of the eye, so this proves nothing"
        );
        let mut expected = camera;
        expected.dolly_at_pixel(1.0 / PER_NOTCH, 600.0, 400.0);
        let result = zoom_at_pixel(&layers, &mut camera, 1.0, 600.0, 400.0);
        assert!(result.anchor.is_none());
        assert_eq!(result.factor, 1.0 / PER_NOTCH);
        assert_eq!(camera.target().x, expected.target().x);
        assert_eq!(camera.target().y, expected.target().y);
        assert_eq!(camera.target().z, expected.target().z);
        assert_eq!(camera.distance(), expected.distance());
    }

    #[test]
    fn one_stray_entity_at_the_origin_no_longer_stops_the_zoom_hundreds_of_metres_from_a_survey() {
        // A survey at MGA coordinates and one point left at (0, 0), with the
        // grid SceneBuilder stands round them, reaching 15% of the 6200 km
        // span past them: the scene's box is 8345 km across. A limit of 1e-4
        // of that held the eye 830 m from the ground, and the near plane - a
        // millionth of the far one, the grid's far corner - was a metre out.
        // What draws the view is good far closer: the point zoomed towards is
        // 6.2e6 m from the origin, where 16 x 2^-53 of that over a tenth of an
        // 800 px view's pixel angle, 2 tan(22.5 deg) / 800, is 0.106 mm.
        let mut layers = SceneLayers::default();
        add_rectangle(
            &mut layers.terrain,
            300000.0,
            6200000.0,
            300200.0,
            6200200.0,
            30.0,
        );
        let stray = layers.entities.add_vertex(Vec3::new(0.0, 0.0, 30.0), INK);
        layers.entities.add_point(stray, 5.0, 1.5);
        layers.datum = 30.0;
        layers.bounds = layers.terrain.bounds();
        layers.bounds.expand(layers.entities.bounds());
        let mut builder = SceneBuilder::new();
        builder.build_grid(&SceneOptions::default(), &mut layers);
        assert!(!layers.grid.empty());
        let mut camera = Camera::default();
        camera.set_viewport_size(1200, 800);
        camera.set_standard_view(StandardView::IsoSouthWest);
        camera.set_target(Vec3::new(300100.0, 6200100.0, 30.0));
        camera.set_distance(400.0);
        let start = plane_under_at(&camera, 600.0, 300.0, 30.0).expect("the survey");
        assert!(
            start.x > 300000.0 && start.x < 300200.0 && start.y > 6200000.0 && start.y < 6200200.0,
            "the cursor is not on the survey"
        );
        let expected =
            16.0 * UNIT_ROUNDOFF * start.length() / (0.1 * 2.0 * 0.41421356237309503 / 800.0);
        near(expected, 1.06e-4, 0.01e-4);
        near(minimum_approach(&camera, start), expected, 1e-9 * expected);
        // From 468 m to 0.106 mm, 1.15^n = 4.4e6 at n = 110.
        for _ in 0..140 {
            let _ = zoom_at_pixel(&layers, &mut camera, 1.0, 600.0, 300.0);
        }
        let under = plane_under_at(&camera, 600.0, 300.0, 30.0).expect("the survey");
        // A depth of 0.1 mm taken as a difference of coordinates 6.2e6 m from
        // the origin, whose unit in the last place is 9.3e-10 m: 1e-5 of it.
        let depth = depth_of(&camera, under);
        near(depth, expected, 1e-4 * expected);
        near((under - start).length(), 0.0, 1e-6);
        // And the view draws it: the near plane stays a thousand times nearer,
        // where a millionth of the far plane alone would lie beyond the
        // ground.
        let mut drawn = camera;
        assert!(drawn.fit_depth_range(scene_depth_box(&layers)));
        assert!(depth / drawn.near_plane() > 990.0);
        assert!(drawn.far_plane() * Camera::NEAR_FAR_FLOOR > 1000.0 * depth);
    }

    #[test]
    fn no_notches_no_number_no_pixel_or_no_viewport_change_nothing() {
        let layers = plane_scene();
        let before = plane_camera(Projection::Perspective);
        let unchanged = |camera: &Camera| {
            camera.target().x == before.target().x
                && camera.target().y == before.target().y
                && camera.target().z == before.target().z
                && camera.distance() == before.distance()
                && camera.orthographic_height() == before.orthographic_height()
        };
        for notches in [0.0, f64::NAN, f64::INFINITY] {
            let mut camera = before;
            let result = zoom_at_pixel(&layers, &mut camera, notches, 700.0, 150.0);
            assert_eq!(result.factor, 1.0);
            assert!(unchanged(&camera), "{notches}");
        }
        let mut camera = before;
        let _ = zoom_at_pixel(&layers, &mut camera, 1.0, f64::NAN, 150.0);
        assert!(unchanged(&camera));
        let mut unsized_camera = before;
        unsized_camera.set_viewport_size(0, 0);
        let _ = zoom_at_pixel(&layers, &mut unsized_camera, 1.0, 700.0, 150.0);
        assert_eq!(unsized_camera.distance(), before.distance());
    }

    #[test]
    fn an_empty_scene_zooms_about_the_targets_plane_down_to_a_tenth_of_a_millimetre() {
        // Nothing to go towards: the target's plane, as before, down to the
        // near plane's limit, 1e-7 / 1e-3 (the target at the origin makes the
        // double arithmetic's bound nothing).
        let empty = SceneLayers::default();
        let mut camera = plane_camera(Projection::Perspective);
        for _ in 0..200 {
            let result = zoom_at_pixel(&empty, &mut camera, 1.0, 600.0, 400.0);
            assert!(result.anchor.is_none());
        }
        near(camera.distance(), 1.0e-4, 1e-15);
    }

    #[test]
    fn the_minimum_approach_is_the_larger_of_the_near_planes_limit_and_the_arithmetics_bound() {
        // tolerance::GEOMETRIC / Camera::NEAR_PIVOT_FLOOR for a point near the
        // origin; 16 x 2^-53 |p| over a tenth of the pixel's angle for one far
        // from it, in proportion to its distance and to the view's height in
        // pixels.
        let mut camera = plane_camera(Projection::Perspective);
        double_eq(minimum_approach(&camera, Vec3::new(0.0, 0.0, 0.0)), 1e-4);
        double_eq(
            minimum_approach(&camera, Vec3::new(3000.0, 4000.0, 0.0)),
            1e-4,
        );
        let angle = 2.0 * 0.41421356237309503 / 800.0;
        let far = Vec3::new(0.0, 6.0e9, 0.0); // an MGA northing in millimetres
        near(
            minimum_approach(&camera, far),
            16.0 * UNIT_ROUNDOFF * 6.0e9 / (0.1 * angle),
            1e-9 * 0.1,
        );
        camera.set_viewport_size(1200, 1600);
        near(
            minimum_approach(&camera, far),
            2.0 * 16.0 * UNIT_ROUNDOFF * 6.0e9 / (0.1 * angle),
            1e-9 * 0.2,
        );
        // A point that is not finite has no bound of its own.
        double_eq(
            minimum_approach(&camera, Vec3::new(f64::NAN, 0.0, 0.0)),
            1e-4,
        );
    }
}
