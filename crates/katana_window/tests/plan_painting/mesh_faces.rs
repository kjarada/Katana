//! The meshes in plan as their faces (mesh_plan_painter; docs/plan_view.md, "Meshes in plan"): a
//! drawing that is nothing but meshes - a design model of kerbs, pavements and batters - reads as a
//! plan, the faces under the drawing, each shaded by its slope, the highest seen where they
//! overlap; Footprint keeps the dashed hulls the plan drew before, and Hidden draws none.
//!
//! Every expected shade is worked by hand from the rule the module documents - a level face at
//! MESH_LEVEL_SHADE (0.65) of its colour, any other at 0.65 x hillshade(n) / hillshade(up) with
//! the 3D views' light - and ink is judged against a baseline painted without the meshes, never as
//! an absolute count of pixels.

use katana_cad::plot::PlotSettings;
use katana_cad::scene::{MeshPlanDisplay, SceneMesh};
use katana_cad::terrain::surface_plan::plan_image_grid;
use katana_core::task_pool::TaskPool;
use katana_entity::{Color as EntityColor, Geometry, Layer, Model};
use katana_geometry::{Box2, Point2, Point3, Polyline2, TriangleMesh};
use katana_render::{Rgba, rgba};
use katana_window::canvas::{Color, Image};
use katana_window::mesh_plan_painter::{MeshInk, MeshPlanCache, mesh_faces_image};
use katana_window::plan_painter::{
    PlanFrame, PlanPaintCache, PlanPaintStats, PlanSource, plan_drawn_bounds,
};

use crate::plan_painter::{
    WHITE, add, entity_of, frame_of, grey, painted_source, painted_source_with, paper_options,
    screen,
};

/// The plan view's ground.
const GROUND: Color = Color::rgb(0x1e, 0x23, 0x29);
const LIGHT_GREY: Rgba = rgba(200, 200, 200);

/// Adds the quad (x0, y0)-(x1, y1) to `mesh` as two faces, its corners at heights `z(x, y)`.
fn quad(mesh: &mut TriangleMesh, x0: f64, y0: f64, x1: f64, y1: f64, z: impl Fn(f64, f64) -> f64) {
    let base = mesh.vertices.len() as u32;
    for (x, y) in [(x0, y0), (x1, y0), (x1, y1), (x0, y1)] {
        mesh.vertices.push(Point3::new(x, y, z(x, y)));
    }
    mesh.faces.push([base, base + 1, base + 2]);
    mesh.faces.push([base, base + 2, base + 3]);
}

/// A level U, 60 units square, open to the north: a bar along the south (y 0 to 20) and two arms
/// (x 0 to 20 and 40 to 60) up to y = 60, so the ground between the arms, x 20 to 40 and y 20 to
/// 60, is inside its hull and not under any face.
fn horseshoe() -> TriangleMesh {
    let mut mesh = TriangleMesh::default();
    quad(&mut mesh, 0.0, 0.0, 60.0, 20.0, |_, _| 0.0);
    quad(&mut mesh, 0.0, 20.0, 20.0, 60.0, |_, _| 0.0);
    quad(&mut mesh, 40.0, 20.0, 60.0, 60.0, |_, _| 0.0);
    mesh
}

fn scene_mesh(mesh: &TriangleMesh, colour: Rgba) -> SceneMesh<'_> {
    SceneMesh {
        name: String::from("model"),
        mesh: Some(mesh),
        flat_color: colour,
        ..SceneMesh::default()
    }
}

/// A frame 100 px square at a pixel a unit about (30, 30): model (x, y) is pixel (50 + x - 30,
/// 50 - y + 30), so the centre of pixel (i, j) is model (i - 19.5, 79.5 - j).
fn frame() -> PlanFrame<'static> {
    frame_of(100.0, 100.0, 1.0, Point2::new(30.0, 30.0), 0.0)
}

fn paint(
    meshes: &[SceneMesh<'_>],
    model: Option<&Model>,
    display: MeshPlanDisplay,
) -> (Image, PlanPaintStats) {
    let source = PlanSource {
        model,
        meshes: Some(meshes),
        mesh_display: display,
        ..PlanSource::default()
    };
    painted_source(&source, &frame(), &screen(), GROUND)
}

/// The largest difference of a channel between two colours.
fn difference(a: Color, b: Color) -> i32 {
    [
        i32::from(a.r) - i32::from(b.r),
        i32::from(a.g) - i32::from(b.g),
        i32::from(a.b) - i32::from(b.b),
    ]
    .into_iter()
    .map(i32::abs)
    .max()
    .unwrap_or(0)
}

/// Whether pixel (x, y) of `image` is inked against `baseline`.
fn inked(image: &Image, baseline: &Image, x: u32, y: u32) -> bool {
    difference(image.pixel(x, y), baseline.pixel(x, y)) > 24
}

mod mesh_faces {
    use super::*;

    #[test]
    fn a_drawing_of_nothing_but_a_mesh_is_inked_where_its_faces_are_and_not_in_the_hole_of_its_hull()
     {
        let mesh = horseshoe();
        let meshes = vec![scene_mesh(&mesh, LIGHT_GREY)];
        let (baseline, _) = paint(&meshes, None, MeshPlanDisplay::Hidden);
        let (image, stats) = paint(&meshes, None, MeshPlanDisplay::Faces);
        // The bar (model 30.5, 9.5), each arm (10.5, 39.5) and (50.5, 39.5): faces.
        for (x, y) in [(50, 70), (30, 40), (70, 40)] {
            assert!(inked(&image, &baseline, x, y), "({x}, {y}) is under a face");
        }
        // The ground between the arms (30.5, 44.5), inside the hull and under no face, and the
        // ground outside the U (-9.5, 29.5): nothing.
        for (x, y) in [(50, 35), (50, 25), (10, 50)] {
            assert!(
                !inked(&image, &baseline, x, y),
                "({x}, {y}) is under no face"
            );
        }
        assert_eq!(stats.meshes_drawn, 1);
        assert_eq!(stats.mesh_triangles_in_view, 6);
        // The faces cover 60 x 20 + 2 x 20 x 40 = 2 800 square units, a pixel each, give or take
        // the rims' pixel either side of an edge.
        assert!(
            (2_700..=3_100).contains(&stats.mesh_pixels_covered),
            "{}",
            stats.mesh_pixels_covered
        );
        // A level face is MESH_LEVEL_SHADE of its colour: 200 x 166 / 255 = 130.2.
        let level = image.pixel(30, 40);
        assert!((i32::from(level.r) - 130).abs() <= 2, "{level:?}");
    }

    #[test]
    fn the_faces_are_beneath_a_line_drawn_over_them() {
        let mesh = horseshoe();
        let meshes = vec![scene_mesh(&mesh, LIGHT_GREY)];
        let mut model = Model::default();
        model
            .layers
            .add(Layer {
                name: String::from("red"),
                color: EntityColor {
                    r: 255,
                    g: 0,
                    b: 0,
                    a: 255,
                },
                ..Layer::default()
            })
            .expect("the layer is added");
        // Along the centres of pixel row 40 (model y = 39.5), across both arms.
        add(
            &mut model,
            entity_of(
                Geometry::Polyline(Polyline2::new(
                    vec![Point2::new(-10.0, 39.5), Point2::new(70.0, 39.5)],
                    false,
                )),
                "red",
            ),
        );
        let (image, stats) = paint(&meshes, Some(&model), MeshPlanDisplay::Faces);
        assert_eq!(stats.entities_drawn, 1);
        for x in [30, 70] {
            let over = image.pixel(x, 40);
            assert!(
                over.r > 200 && over.g < 60 && over.b < 60,
                "({x}, 40) {over:?}"
            );
            // And the face just off the line is the face's grey.
            let beside = image.pixel(x, 45);
            assert!(
                (i32::from(beside.g) - 130).abs() <= 2,
                "({x}, 45) {beside:?}"
            );
        }
    }

    #[test]
    fn a_face_turned_from_the_sun_is_darker_and_one_turned_to_it_lighter_than_a_level_one() {
        // Three 20-unit squares in one colour: level; falling 1 in 2 to the south-east, z = (y -
        // x) / 2; and falling 1 in 2 to the north-west, z = (x - y) / 2. The 3D views' light is a
        // sun 45 degrees up in the north-west, L = (-0.5, 0.5, 0.7071), with 0.3 sky, 0.1 ground
        // and 0.8 sun: hillshade(n) = 0.1 + 0.2 (0.5 + 0.5 n.z) + 0.8 max(0, n.L), 0.865685 for
        // a level face.
        // South-east: n = (0.5, -0.5, 1) / 1.224745 = (0.40825, -0.40825, 0.81650); n.L = 0.16910;
        // hillshade = 0.28165 + 0.13528 = 0.41693; 0.65 x 0.41693 / 0.865685 = 0.31306, kept as
        // 80 / 255; 200 x 80 / 255 = 62.7.
        // North-west: n.L = 0.98560 and hillshade 1.07 is held at 1: 0.65 / 0.865685 = 0.75085,
        // kept as 191 / 255; 200 x 191 / 255 = 149.8.
        // Level: 0.65, kept as 166 / 255; 130.2.
        let mut mesh = TriangleMesh::default();
        quad(&mut mesh, 0.0, 0.0, 20.0, 20.0, |_, _| 0.0);
        quad(&mut mesh, 25.0, 0.0, 45.0, 20.0, |x, y| 0.5 * (y - x));
        quad(&mut mesh, 0.0, 30.0, 20.0, 50.0, |x, y| 0.5 * (x - y));
        let meshes = vec![scene_mesh(&mesh, LIGHT_GREY)];
        let (image, _) = paint(&meshes, None, MeshPlanDisplay::Faces);
        // Centres: level (10.5, 9.5) is pixel (30, 70); south-east (35.5, 9.5) is (55, 70);
        // north-west (10.5, 39.5) is (30, 40).
        let level = i32::from(image.pixel(30, 70).g);
        let away = i32::from(image.pixel(55, 70).g);
        let towards = i32::from(image.pixel(30, 40).g);
        assert!((level - 130).abs() <= 1, "level {level}");
        assert!((away - 63).abs() <= 1, "south-east {away}");
        assert!((towards - 150).abs() <= 1, "north-west {towards}");
    }

    #[test]
    fn where_faces_overlap_the_highest_is_seen_whatever_the_order() {
        // A red square at z = 5 and a blue one at z = 1 over the same ground, the lower listed
        // last; and the same two the other way round. A plan looks down: red either way.
        let mut high = TriangleMesh::default();
        quad(&mut high, 0.0, 0.0, 40.0, 40.0, |_, _| 5.0);
        let mut low = TriangleMesh::default();
        quad(&mut low, 10.0, 10.0, 50.0, 50.0, |_, _| 1.0);
        for meshes in [
            vec![
                scene_mesh(&high, rgba(255, 0, 0)),
                scene_mesh(&low, rgba(0, 0, 255)),
            ],
            vec![
                scene_mesh(&low, rgba(0, 0, 255)),
                scene_mesh(&high, rgba(255, 0, 0)),
            ],
        ] {
            let (image, _) = paint(&meshes, None, MeshPlanDisplay::Faces);
            // (25.5, 24.5): under both.
            let both = image.pixel(45, 55);
            assert!(both.r > 150 && both.b < 40, "{both:?}");
            // (45.5, 44.5): under the low one only.
            let low_only = image.pixel(65, 35);
            assert!(low_only.b > 150 && low_only.r < 40, "{low_only:?}");
        }
    }

    #[test]
    fn a_mesh_narrower_than_a_pixel_still_leaves_a_line() {
        // A kerb 0.2 units wide and 40 long, north-south at x = 10.25, at a pixel a unit: no
        // pixel's centre is inside it, and every row it crosses shows it.
        let mut kerb = TriangleMesh::default();
        quad(&mut kerb, 10.15, 0.0, 10.35, 40.0, |_, _| 0.15);
        let meshes = vec![scene_mesh(&kerb, LIGHT_GREY)];
        let (baseline, _) = paint(&meshes, None, MeshPlanDisplay::Hidden);
        let (image, _) = paint(&meshes, None, MeshPlanDisplay::Faces);
        // Rows 41 to 78 (model y 38.5 to 1.5), the pixel column at x = 10.25: 50 + 10.25 - 30 is
        // column 30.
        for y in 41..=78 {
            assert!(inked(&image, &baseline, 30, y), "row {y}");
            assert!(!inked(&image, &baseline, 33, y), "row {y} is not smeared");
        }
    }

    #[test]
    fn footprint_draws_the_hull_as_before_and_no_faces() {
        let mesh = horseshoe();
        let meshes = vec![scene_mesh(&mesh, LIGHT_GREY)];
        let (baseline, _) = paint(&meshes, None, MeshPlanDisplay::Hidden);
        let (image, stats) = paint(&meshes, None, MeshPlanDisplay::Footprint);
        // The hull is the 60-unit square filled faintly (alpha 40): the hole between the arms is
        // filled as the arm is, the same colour, and both differ from the ground.
        assert_eq!(image.pixel(50, 35), image.pixel(30, 40));
        assert_ne!(image.pixel(50, 35), baseline.pixel(50, 35));
        assert!(
            difference(image.pixel(30, 40), GROUND) < 40,
            "a faint fill, not a face"
        );
        // Its north side, across the open end of the U (model y = 60, pixel row 20), where no
        // face is, is the outline. (Its dashes in paper millimetres are plan_painter.rs's
        // a_mesh_footprint_on_paper_is_outlined_in_paper_millimetres_at_every_resolution.)
        let along = (32..68)
            .filter(|&x| difference(image.pixel(x, 20), image.pixel(x, 22)) > 24)
            .count();
        assert!(along > 0, "the outline is drawn");
        assert_eq!(stats.meshes_drawn, 0, "no faces");
        assert_eq!(stats.mesh_images_made, 0);
    }

    #[test]
    fn hidden_draws_nothing_of_the_meshes_and_zoom_extents_frames_none_of_them() {
        let mesh = horseshoe();
        let meshes = vec![scene_mesh(&mesh, LIGHT_GREY)];
        let (hidden, stats) = paint(&meshes, None, MeshPlanDisplay::Hidden);
        let (none, _) = paint(&[], None, MeshPlanDisplay::Faces);
        assert_eq!(hidden, none);
        assert_eq!(stats.meshes_drawn, 0);
        let model = Model::default();
        let bounds_with = |display: MeshPlanDisplay| {
            let source = PlanSource {
                model: Some(&model),
                meshes: Some(&meshes),
                mesh_display: display,
                ..PlanSource::default()
            };
            plan_drawn_bounds(&source, &Default::default(), &Default::default())
        };
        let faces = bounds_with(MeshPlanDisplay::Faces);
        assert_eq!(
            (faces.min, faces.max),
            (Point2::new(0.0, 0.0), Point2::new(60.0, 60.0)),
            "a drawing of nothing but meshes frames them"
        );
        assert_eq!(bounds_with(MeshPlanDisplay::Footprint), faces);
        assert!(bounds_with(MeshPlanDisplay::Hidden).empty());
    }

    #[test]
    fn the_faces_image_is_kept_until_the_view_or_the_meshes_change() {
        let mesh = horseshoe();
        let meshes = vec![scene_mesh(&mesh, LIGHT_GREY)];
        let source = PlanSource {
            meshes: Some(&meshes),
            ..PlanSource::default()
        };
        let mut cache = PlanPaintCache::default();
        let (first, made) = painted_source_with(&source, &frame(), &screen(), GROUND, &mut cache);
        assert_eq!(made.mesh_images_made, 1);
        // The same view again - a selection, a command that touches no mesh: laid down, not made.
        let (again, kept) = painted_source_with(&source, &frame(), &screen(), GROUND, &mut cache);
        assert_eq!(kept.mesh_images_made, 0);
        assert_eq!(again, first);
        assert_eq!(kept.mesh_pixels_covered, made.mesh_pixels_covered);
        assert_eq!(cache.mesh_cache().images_made(), 1);
        assert_eq!(cache.mesh_cache().displays_made(), 1);
        // Panned by five pixels with the whole mesh still in view: the image is of the model, on
        // the same pixel grid, so it is laid down again five pixels over and not made. The left
        // arm's (10.5, 39.5) was pixel (30, 40) and is (25, 40).
        let panned = frame_of(100.0, 100.0, 1.0, Point2::new(35.0, 30.0), 0.0);
        let (shifted, moved) =
            painted_source_with(&source, &panned, &screen(), GROUND, &mut cache);
        assert_eq!(moved.mesh_images_made, 0);
        assert_eq!(shifted.pixel(25, 40), first.pixel(30, 40));
        assert_eq!(shifted.pixel(13, 40), GROUND, "west of the arm, now at pixel 15");
        // Panned so the mesh runs off the view (x 10 to 110 shown): what is in view is made again,
        // the mesh's shades and rim kept.
        let off = frame_of(100.0, 100.0, 1.0, Point2::new(60.0, 30.0), 0.0);
        let (_, clipped) = painted_source_with(&source, &off, &screen(), GROUND, &mut cache);
        assert_eq!(clipped.mesh_images_made, 1);
        assert_eq!(cache.mesh_cache().images_made(), 2);
        assert_eq!(cache.mesh_cache().displays_made(), 1);
        // Zoomed: made again.
        let zoomed = frame_of(100.0, 100.0, 2.0, Point2::new(35.0, 30.0), 0.0);
        let (_, closer) = painted_source_with(&source, &zoomed, &screen(), GROUND, &mut cache);
        assert_eq!(closer.mesh_images_made, 1);
        // Recoloured: made again, in the new colour.
        let red = vec![scene_mesh(&mesh, rgba(255, 0, 0))];
        let recoloured = PlanSource {
            meshes: Some(&red),
            ..PlanSource::default()
        };
        let (image, after) =
            painted_source_with(&recoloured, &zoomed, &screen(), GROUND, &mut cache);
        assert_eq!(after.mesh_images_made, 1);
        assert_eq!(cache.mesh_cache().images_made(), 4);
        // The left arm at (10.5, 39.5): pixel (50 + 2 (10.5 - 35), 50 - 2 (39.5 - 30)) = (1, 31).
        assert!(image.pixel(1, 31).r > 120 && image.pixel(1, 31).g < 30);
    }

    #[test]
    fn the_faces_image_is_the_same_on_one_thread_and_on_eight() {
        // 40 meshes of 400 random faces each over a 300 x 200 image: overlapping faces at
        // different heights, faces across the 32-row bands' borders, and slivers.
        let mut state: u64 = 11;
        let mut next = || {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (state >> 11) as f64 * (1.0 / 9_007_199_254_740_992.0)
        };
        let mut meshes = Vec::new();
        for _ in 0..40 {
            let mut mesh = TriangleMesh::default();
            let (cx, cy) = (300.0 * next(), 200.0 * next());
            for f in 0..400u32 {
                for _ in 0..3 {
                    mesh.vertices.push(Point3::new(
                        cx + 60.0 * (next() - 0.5),
                        cy + 60.0 * (next() - 0.5),
                        10.0 * next(),
                    ));
                }
                mesh.faces.push([3 * f, 3 * f + 1, 3 * f + 2]);
            }
            meshes.push(mesh);
        }
        let colours: Vec<Rgba> = (0..40u8)
            .map(|k| rgba(k.wrapping_mul(53), k.wrapping_mul(97), k.wrapping_mul(29)))
            .collect();
        let items: Vec<SceneMesh<'_>> = meshes
            .iter()
            .zip(&colours)
            .map(|(mesh, &colour)| scene_mesh(mesh, colour))
            .collect();
        let shown: Vec<&SceneMesh<'_>> = items.iter().collect();
        let visible = Box2::new(Point2::new(0.0, 0.0), Point2::new(300.0, 200.0));
        let mut bounds = Box2::default();
        for mesh in &meshes {
            for vertex in &mesh.vertices {
                bounds.expand(Point2::new(vertex.x, vertex.y));
            }
        }
        let grid = plan_image_grid(&visible, &bounds, 1.0, 16_000_000);
        assert!(grid.height > 3 * 32, "the premise: several bands");
        let one = TaskPool::new(0);
        let eight = TaskPool::new(7);
        let (alone, stats) = mesh_faces_image(
            &shown,
            &grid,
            MeshInk::SCREEN,
            &mut MeshPlanCache::new(),
            &one,
        );
        let (together, _) = mesh_faces_image(
            &shown,
            &grid,
            MeshInk::SCREEN,
            &mut MeshPlanCache::new(),
            &eight,
        );
        assert!(
            stats.triangles_in_view > 10_000,
            "{}",
            stats.triangles_in_view
        );
        assert!(stats.pixels_covered > 10_000, "{}", stats.pixels_covered);
        assert!(
            alone == together,
            "the image differs between one thread and eight"
        );
    }

    #[test]
    fn faces_print_on_paper_under_the_plot_colour_rule_and_leave_the_hole_white() {
        // A white mesh prints black (white_to_black), at MESH_FACE_PAPER_OPACITY over the
        // paper: about half grey. The hole between the arms is paper.
        let mesh = horseshoe();
        let meshes = vec![scene_mesh(&mesh, rgba(255, 255, 255))];
        let source = PlanSource {
            meshes: Some(&meshes),
            ..PlanSource::default()
        };
        let settings = PlotSettings::default();
        let (image, stats) =
            painted_source(&source, &frame(), &paper_options(&settings, 4.0), WHITE);
        assert_eq!(stats.meshes_drawn, 1);
        let arm = grey(image.pixel(30, 40));
        assert!((100..=160).contains(&arm), "{arm}");
        assert_eq!(image.pixel(50, 35), WHITE);
    }

    #[test]
    fn nothing_to_draw_draws_nothing() {
        // No meshes, an empty mesh, and a mesh whose vertices are not finite.
        let empty = TriangleMesh::default();
        let mut broken = TriangleMesh::default();
        quad(&mut broken, 0.0, 0.0, 10.0, 10.0, |_, _| f64::NAN);
        broken.vertices[0].x = f64::INFINITY;
        broken.vertices[2].y = f64::NAN;
        let (ground, _) = paint(&[], None, MeshPlanDisplay::Faces);
        let meshes = vec![
            scene_mesh(&empty, LIGHT_GREY),
            scene_mesh(&broken, LIGHT_GREY),
        ];
        let (image, stats) = paint(&meshes, None, MeshPlanDisplay::Faces);
        assert_eq!(stats.mesh_triangles_in_view, 0);
        assert_eq!(image, ground);
    }
}
