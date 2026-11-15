// Document -> DrawList (PLAN.MD Phase 15, Rule 3; tests/cad/test_scene.cpp).
//
// The assertions worth making about a scene builder are structural: that the
// renderer is given the geometry the model holds, that it is given it ONCE
// rather than per-triangle, that a tessellation honours its tolerance, and
// that building a scene cannot modify the document.

use crate::{near, ok, tin};
use katana_cad::document::Document;
use katana_cad::layer_overrides::LayerOverrides;
use katana_cad::scene::{
    LineworkHeights, SceneBuilder, SceneLayers, SceneMesh, SceneOptions, SceneSurface,
    SurfaceColoring, elevation_ramp_color, render_layers, scene_bounds,
};
use katana_cad::terrain::surface_style::{DENSE_SURFACE_TRIANGLES, SurfaceStyle};
use katana_commands::{
    EntityAttributes, create_arc, create_circle, create_entities, create_layer, create_line,
    create_linetype, create_point, create_polyline, delete_entities, set_entity_visible,
    update_layer,
};
use katana_core::task_pool::TaskPool;
use katana_entity::entity::set_heights;
use katana_entity::{Color, Entity, EntityId, Geometry, Layer, Linetype, LinetypeElement};
use katana_geometry::{Arc2, Point2, Point3, Polyline2, Segment2, TriangleMesh};
use katana_math::Vec3;
use katana_render::{
    Camera, DrawList, Framebuffer, Projection, Rasterizer, RenderOptions, Rgba, StandardView, rgba,
    shade,
};
use katana_terrain::TinSurface;

/// Two triangles over a 100 x 100 square, rising 10 to the east.
fn ramp() -> TinSurface {
    tin(
        vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(100.0, 0.0, 10.0),
            Point3::new(100.0, 100.0, 10.0),
            Point3::new(0.0, 100.0, 0.0),
        ],
        vec![[0, 1, 2], [0, 2, 3]],
    )
}

/// The grid would swamp the counts below.
fn plain_options() -> SceneOptions<'static> {
    SceneOptions {
        draw_grid: false,
        ..SceneOptions::default()
    }
}

fn with_surface(surface: &TinSurface, style: SurfaceStyle) -> SceneSurface<'_> {
    SceneSurface {
        surface: Some(surface),
        style,
        ..SceneSurface::default()
    }
}

fn line(document: &mut Document, from: Point2, to: Point2) {
    let attributes = document.current_attributes();
    ok(document.execute(create_line(from, to, attributes)));
}

fn last_id(document: &Document) -> EntityId {
    *document.model().entities.ids().last().expect("an entity")
}

fn layer_named(name: &str) -> Layer {
    Layer {
        name: String::from(name),
        ..Layer::default()
    }
}

/// A 3D string from (0, 50) to (100, 50) with its own heights 30 and 40.
fn three_d_string() -> Entity {
    let mut entity = Entity {
        geometry: Geometry::Polyline(Polyline2::new(
            vec![Point2::new(0.0, 50.0), Point2::new(100.0, 50.0)],
            false,
        )),
        ..Entity::default()
    };
    set_heights(&mut entity.properties, &[Some(30.0), Some(40.0)]);
    entity
}

/// Terrain, drawing, selection and grid as the 3D view builds them.
fn layers_of(
    document: &Document,
    surfaces: &[SceneSurface<'_>],
    options: &SceneOptions<'_>,
) -> SceneLayers {
    let mut builder = SceneBuilder::new();
    let mut layers = SceneLayers::default();
    builder.build_terrain(surfaces, &[], options, &mut layers);
    builder.build_entities(document, surfaces, options, &mut layers);
    builder.build_selection(document, surfaces, options, &mut layers);
    builder.build_grid(options, &mut layers);
    layers
}

/// A flat square TIN at height z, 100 x 100 from (x0, y0).
fn flat_at(x0: f64, y0: f64, z: f64) -> TinSurface {
    tin(
        vec![
            Point3::new(x0, y0, z),
            Point3::new(x0 + 100.0, y0, z),
            Point3::new(x0 + 100.0, y0 + 100.0, z),
            Point3::new(x0, y0 + 100.0, z),
        ],
        vec![[0, 1, 2], [0, 2, 3]],
    )
}

fn dashed_linetype() -> Linetype {
    Linetype {
        name: String::from("dashed"),
        pattern: vec![
            LinetypeElement { length: 1.0 },
            LinetypeElement { length: -0.5 },
        ],
        ..Linetype::default()
    }
}

/// Puts a dashed linetype on a layer and makes it the document's current one.
fn use_dashed_layer(document: &mut Document) {
    ok(document.execute(create_linetype(dashed_linetype())));
    let layer = Layer {
        name: String::from("fence"),
        linetype: String::from("dashed"),
        ..Layer::default()
    };
    ok(document.execute(create_layer(layer)));
    ok(document.set_current_layer("fence"));
}

/// A tetrahedron: four vertices, four faces, nothing shared with a surface.
fn tetrahedron() -> TriangleMesh {
    TriangleMesh {
        vertices: vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(10.0, 0.0, 0.0),
            Point3::new(0.0, 10.0, 0.0),
            Point3::new(0.0, 0.0, 6.0),
        ],
        faces: vec![[0, 2, 1], [0, 1, 3], [1, 2, 3], [2, 0, 3]],
    }
}

fn mesh_of(mesh: &TriangleMesh, style: SurfaceStyle) -> SceneMesh<'_> {
    SceneMesh {
        mesh: Some(mesh),
        style,
        ..SceneMesh::default()
    }
}

mod cad_scene {
    use super::*;

    #[test]
    fn surface_vertices_are_shared_so_the_transform_runs_once_per_vertex() {
        // The whole reason DrawList is indexed. An unshaded wireframe of a
        // 4-vertex surface must submit 4 vertices, not 3 per triangle.
        let surface = ramp();
        let document = Document::default();
        let mut builder = SceneBuilder::new();
        let mut list = DrawList::default();
        let item = SceneSurface {
            name: String::from("Existing"),
            coloring: SurfaceColoring::Flat,
            ..with_surface(&surface, SurfaceStyle::Wireframe)
        };
        let options = SceneOptions {
            draw_entities: false,
            ..plain_options()
        };
        builder.build(&document, &[item], &options, &mut list, &[]);
        assert!(
            list.triangles.is_empty(),
            "wireframe draws no filled triangles"
        );
        assert_eq!(
            list.lines.len(),
            5,
            "two triangles share one edge: 3 + 3 - 1 = 5"
        );
    }

    #[test]
    fn shaded_surfaces_produce_one_triangle_per_facet() {
        let surface = ramp();
        let document = Document::default();
        let mut builder = SceneBuilder::new();
        let mut list = DrawList::default();
        let options = SceneOptions {
            draw_entities: false,
            ..plain_options()
        };
        builder.build(
            &document,
            &[with_surface(&surface, SurfaceStyle::Shaded)],
            &options,
            &mut list,
            &[],
        );
        assert_eq!(list.triangles.len(), surface.triangle_count());
        assert!(list.lines.is_empty());
        assert!(list.all_finite());
    }

    #[test]
    fn a_hidden_or_invisible_surface_contributes_nothing() {
        let surface = ramp();
        let document = Document::default();
        let mut builder = SceneBuilder::new();
        let mut list = DrawList::default();
        let options = SceneOptions {
            draw_entities: false,
            ..plain_options()
        };

        let mut item = with_surface(&surface, SurfaceStyle::Hidden);
        builder.build(
            &document,
            std::slice::from_ref(&item),
            &options,
            &mut list,
            &[],
        );
        assert!(list.empty());

        item.style = SurfaceStyle::Shaded;
        item.visible = false;
        builder.build(
            &document,
            std::slice::from_ref(&item),
            &options,
            &mut list,
            &[],
        );
        assert!(list.empty());

        item.surface = None;
        item.visible = true;
        builder.build(&document, &[item], &options, &mut list, &[]);
        assert!(
            list.empty(),
            "a missing surface must be skipped, not dereferenced"
        );
    }

    #[test]
    fn vertical_exaggeration_scales_about_the_datum_and_nothing_else() {
        let surface = ramp(); // z from 0 to 10
        let document = Document::default();
        let mut builder = SceneBuilder::new();
        let mut list = DrawList::default();
        let options = SceneOptions {
            draw_entities: false,
            vertical_exaggeration: 5.0,
            exaggeration_datum: 2.0,
            ..plain_options()
        };
        let item = with_surface(&surface, SurfaceStyle::Wireframe);
        builder.build(
            &document,
            std::slice::from_ref(&item),
            &options,
            &mut list,
            &[],
        );

        let bounds = list.bounds();
        assert!(!bounds.empty());
        // z = datum + (z - datum) * 5: 0 -> -8, 10 -> 42.
        near(bounds.min.z, -8.0, 1e-9);
        near(bounds.max.z, 42.0, 1e-9);
        // Plan coordinates must be untouched: exaggerating height is not a
        // licence to move anything horizontally.
        near(bounds.min.x, 0.0, 1e-9);
        near(bounds.max.x, 100.0, 1e-9);

        // And scene_bounds must agree with what was actually built, or
        // framing the view would cut off the top of the exaggerated surface.
        let declared = scene_bounds(&document, &[item], &options, &[]);
        near(declared.min.z, bounds.min.z, 1e-9);
        near(declared.max.z, bounds.max.z, 1e-9);
    }

    #[test]
    fn an_invalid_exaggeration_falls_back_to_one_rather_than_collapsing_the_scene() {
        let surface = ramp();
        let document = Document::default();
        let mut builder = SceneBuilder::new();
        let mut list = DrawList::default();
        let item = with_surface(&surface, SurfaceStyle::Wireframe);
        for bad in [0.0, -3.0, f64::NAN, f64::INFINITY] {
            let options = SceneOptions {
                draw_entities: false,
                vertical_exaggeration: bad,
                ..plain_options()
            };
            builder.build(
                &document,
                std::slice::from_ref(&item),
                &options,
                &mut list,
                &[],
            );
            let bounds = list.bounds();
            assert!(!bounds.empty(), "exaggeration {bad}");
            near(bounds.min.z, 0.0, 1e-9);
            near(bounds.max.z, 10.0, 1e-9);
        }
    }

    #[test]
    fn drawing_entities_does_not_modify_the_document() {
        // Rule 3, asserted rather than assumed: the renderer path is
        // read-only.
        let mut document = Document::default();
        line(&mut document, Point2::new(0.0, 0.0), Point2::new(10.0, 5.0));
        let before = document.model().entities.ids();
        let next_before = document.model().entities.next_id();

        let mut builder = SceneBuilder::new();
        let mut list = DrawList::default();
        builder.build(&document, &[], &plain_options(), &mut list, &[]);

        assert_eq!(document.model().entities.ids(), before);
        assert_eq!(
            document.model().entities.next_id(),
            next_before,
            "building a scene must not consume an entity id"
        );
        assert!(!list.lines.is_empty());
    }

    #[test]
    fn hidden_entities_are_not_drawn() {
        let mut document = Document::default();
        line(&mut document, Point2::new(0.0, 0.0), Point2::new(10.0, 5.0));
        let mut builder = SceneBuilder::new();
        let mut visible = DrawList::default();
        builder.build(&document, &[], &plain_options(), &mut visible, &[]);
        assert_eq!(visible.lines.len(), 1);

        // Hide it through a command, as the application would.
        let id = last_id(&document);
        ok(document.execute(set_entity_visible(vec![id], false)));

        let mut after = DrawList::default();
        builder.build(&document, &[], &plain_options(), &mut after, &[]);
        assert!(after.lines.is_empty());
    }

    #[test]
    fn rebuilding_into_the_same_list_does_not_accumulate() {
        // The list is reused every frame; a build that appended instead of
        // replacing would grow without bound and look like a memory leak.
        let mut document = Document::default();
        line(&mut document, Point2::new(0.0, 0.0), Point2::new(10.0, 5.0));
        let mut builder = SceneBuilder::new();
        let mut list = DrawList::default();
        builder.build(&document, &[], &plain_options(), &mut list, &[]);
        let first = list.positions.len();
        for _ in 0..5 {
            builder.build(&document, &[], &plain_options(), &mut list, &[]);
        }
        assert_eq!(list.positions.len(), first);
    }

    #[test]
    fn selected_entities_are_drawn_in_the_selection_colour() {
        let mut document = Document::default();
        line(&mut document, Point2::new(0.0, 0.0), Point2::new(10.0, 5.0));
        let id = last_id(&document);

        let options = plain_options();
        let mut builder = SceneBuilder::new();
        let mut list = DrawList::default();
        builder.build(&document, &[], &options, &mut list, &[]);
        assert!(!list.colors.is_empty());
        assert_ne!(list.colors[0], options.selection_color);

        document.selection_mut().add(id);
        builder.build(&document, &[], &options, &mut list, &[]);
        assert!(!list.colors.is_empty());
        assert_eq!(list.colors[0], options.selection_color);
    }

    #[test]
    fn the_one_list_build_draws_a_selected_line_at_the_selections_one_width() {
        // One width for a selected line in every build: the selection's core,
        // 3 px (selection_style.rs). The one-list build drew it at 2 px, from
        // an option of its own that no view read.
        let mut document = Document::default();
        line(&mut document, Point2::new(0.0, 0.0), Point2::new(10.0, 5.0));
        let options = plain_options();
        let mut builder = SceneBuilder::new();
        let mut list = DrawList::default();
        builder.build(&document, &[], &options, &mut list, &[]);
        assert_eq!(list.lines.len(), 1);
        assert_eq!(list.lines[0].width, 1.0, "unselected, the entity width");

        let id = last_id(&document);
        document.selection_mut().add(id);
        builder.build(&document, &[], &options, &mut list, &[]);
        assert_eq!(list.lines.len(), 1);
        assert_eq!(list.lines[0].width, 3.0);
    }

    #[test]
    fn the_grid_is_bounded_however_fine_a_spacing_is_asked_for() {
        let document = Document::default();
        let mut builder = SceneBuilder::new();
        let mut list = DrawList::default();
        let options = SceneOptions {
            draw_entities: false,
            draw_grid: true,
            grid_spacing: 1.0e-6, // nonsense: 10^8 cells across a 100 m patch
            ..SceneOptions::default()
        };
        builder.build(&document, &[], &options, &mut list, &[]);

        // The spacing is doubled until at most 200 cells span each side, so
        // at most 201 lines each way of at most 200 cells each: 2 * 201 * 200.
        assert!(list.lines.len() <= 2 * 201 * 200);
        assert!(!list.lines.is_empty());
    }

    #[test]
    fn an_automatic_grid_is_sized_to_the_scene_and_cut_into_cells() {
        // Nothing to draw, so the grid stands round the default 100 m patch
        // (-50..50 each way). Worked by hand from emit_grid's rule:
        //   span 100, 16 cells wanted -> 6.25 m -> the 1-2-5 step above it,
        //   10 m; reach 15% of the span = 15 m, so x runs over -65..65:
        //   floor(-6.5) = -7 .. ceil(6.5) = 7, 14 cells and 15 lines each way;
        //   one DrawLine per cell: 15 lines x 14 cells x 2 directions = 420.
        // A full-length line per grid line would be 30; cut per cell it is
        // 420, which is what keeps a long diagonal from touching every tile.
        let document = Document::default();
        let mut builder = SceneBuilder::new();
        let mut list = DrawList::default();
        let options = SceneOptions {
            draw_entities: false,
            draw_grid: true,
            ..SceneOptions::default()
        };
        builder.build(&document, &[], &options, &mut list, &[]);
        assert_eq!(list.lines.len(), 420);
        let bounds = list.bounds();
        near(bounds.min.x, -70.0, 1e-9);
        near(bounds.max.x, 70.0, 1e-9);

        // A scene 100 times larger gets a grid 100 times coarser and the same
        // number of cells, not a sea of 10 m lines.
        let big = tin(
            vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(10000.0, 0.0, 10.0),
                Point3::new(10000.0, 10000.0, 10.0),
                Point3::new(0.0, 10000.0, 0.0),
            ],
            vec![[0, 1, 2], [0, 2, 3]],
        );
        builder.build(
            &document,
            &[with_surface(&big, SurfaceStyle::Shaded)],
            &options,
            &mut list,
            &[],
        );
        // span 10 000 -> 625 -> 1000 m cells; reach 1500: -1500..11500 is
        // floor(-1.5) = -2 .. ceil(11.5) = 12, 14 cells, 15 lines: 420 again.
        assert_eq!(list.lines.len(), 420);
    }

    #[test]
    fn the_grid_draws_its_axes_last_so_no_other_grid_line_breaks_them() {
        // The grid writes no depth (render_layers), so where two of its lines
        // cross the later covers the earlier. Round the default 100 m patch
        // the origin is in range (-70..70,
        // AnAutomaticGridIsSizedToTheSceneAndCutIntoCells), so both axes are
        // drawn, 2 px against every other line's 1: 14 cells each, the last
        // 2 x 14 = 28 of the 420 lines, and none before them.
        let document = Document::default();
        let mut builder = SceneBuilder::new();
        let mut list = DrawList::default();
        let options = SceneOptions {
            draw_entities: false,
            draw_grid: true,
            ..SceneOptions::default()
        };
        builder.build(&document, &[], &options, &mut list, &[]);
        assert_eq!(list.lines.len(), 420);
        for (k, line) in list.lines.iter().enumerate() {
            assert_eq!(line.width, if k < 392 { 1.0 } else { 2.0 }, "line {k}");
        }
    }

    #[test]
    fn the_grid_stands_on_the_lowest_surface() {
        // A surface from 0 to 10 lifted to 250..260: the grid goes under it at
        // 250, where the ground is, not at a z = 0 two hundred metres below.
        // Exactly in the plane of the surface's lowest part; drawn as a
        // backdrop it never shows through it
        // (SceneFrame.AFlatSurfaceAtTheDatumCoversTheGridStandingUnderIt).
        let surface = tin(
            vec![
                Point3::new(0.0, 0.0, 250.0),
                Point3::new(100.0, 0.0, 260.0),
                Point3::new(100.0, 100.0, 260.0),
                Point3::new(0.0, 100.0, 250.0),
            ],
            vec![[0, 1, 2], [0, 2, 3]],
        );
        let item = with_surface(&surface, SurfaceStyle::Shaded);
        let document = Document::default();
        let mut builder = SceneBuilder::new();
        let mut layers = SceneLayers::default();
        let options = SceneOptions::default();
        builder.build_terrain(std::slice::from_ref(&item), &[], &options, &mut layers);
        builder.build_entities(
            &document,
            std::slice::from_ref(&item),
            &options,
            &mut layers,
        );
        builder.build_grid(&options, &mut layers);
        assert!(!layers.grid.lines.is_empty());
        let bounds = layers.grid.bounds();
        near(bounds.min.z, 250.0, 1e-9);
        near(bounds.max.z, 250.0, 1e-9);
    }

    #[test]
    fn scene_bounds_cover_both_surfaces_and_drawing_geometry() {
        let surface = ramp();
        let mut document = Document::default();
        line(
            &mut document,
            Point2::new(-50.0, -50.0),
            Point2::new(-10.0, -10.0),
        );
        let item = SceneSurface {
            surface: Some(&surface),
            ..SceneSurface::default()
        };
        let bounds = scene_bounds(&document, &[item], &plain_options(), &[]);
        assert!(!bounds.empty());
        // The drawing extends further west than the surface, the surface
        // further east than the drawing.
        near(bounds.min.x, -50.0, 1e-9);
        near(bounds.max.x, 100.0, 1e-9);
        near(bounds.max.z, 10.0, 1e-9);
    }

    #[test]
    fn by_layer_entities_are_drawn_in_their_layers_colour() {
        // The 3D view used to read the entity's own colour or a fixed default
        // and ignore the layer entirely. ByLayer is the DEFAULT for a new
        // entity, so everything drawn on a coloured layer came out the default
        // grey while the 2D view showed it correctly - the two views disagreed
        // about the common case, not a corner one.
        let mut document = Document::default();
        let kerb = Layer {
            name: String::from("kerb"),
            color: Color::new(255, 0, 0, 255),
            ..Layer::default()
        };
        ok(document.execute(create_layer(kerb)));
        ok(document.set_current_layer("kerb"));
        line(&mut document, Point2::new(0.0, 0.0), Point2::new(10.0, 5.0));

        let options = SceneOptions {
            default_color: rgba(1, 2, 3), // deliberately not red
            ..plain_options()
        };
        let mut builder = SceneBuilder::new();
        let mut list = DrawList::default();
        builder.build(&document, &[], &options, &mut list, &[]);

        assert!(!list.colors.is_empty());
        assert_eq!(
            list.colors[0],
            katana_render::rgba_with_alpha(255, 0, 0, 255),
            "a ByLayer entity must take its layer's colour, not the scene default"
        );
        assert_ne!(list.colors[0], options.default_color);
    }

    // There is deliberately no scene test for a named style overriding the
    // layer, because no command creates a Style: the table can only be
    // populated by loading a project that already contains one.
    // resolve_display's own tests cover the chain.

    /// How many lines the one-list build draws of what `draw` puts on a
    /// dashed layer.
    fn line_count(
        builder: &mut SceneBuilder,
        options: &SceneOptions<'_>,
        draw: impl FnOnce(&mut Document),
    ) -> usize {
        let mut document = Document::default();
        use_dashed_layer(&mut document);
        draw(&mut document);
        let mut list = DrawList::default();
        builder.build(&document, &[], options, &mut list, &[]);
        list.lines.len()
    }

    #[test]
    fn every_curve_kind_is_dashed_including_a_plain_line() {
        // The half-feature this catches: a Segment2 that adds its DrawLine
        // directly instead of going through the shared emitter stays SOLID in
        // 3D while polylines, arcs and circles dash. No compiler complains,
        // and it is the most confusing possible outcome - the same linetype
        // works on some entities and not others.
        let options = plain_options();
        let mut builder = SceneBuilder::new();

        // A 20 m run of a 1.5 m period is about 14 dashes; anything that
        // stayed solid would be exactly 1 line (or 4 for the closed shapes).
        let straight = line_count(&mut builder, &options, |d| {
            line(d, Point2::new(0.0, 0.0), Point2::new(20.0, 0.0));
        });
        assert!(
            straight > 5,
            "a plain Segment2 must dash too, not stay solid"
        );

        let polyline = line_count(&mut builder, &options, |d| {
            let p = Polyline2::new(
                vec![
                    Point2::new(0.0, 0.0),
                    Point2::new(20.0, 0.0),
                    Point2::new(20.0, 20.0),
                ],
                false,
            );
            let attributes = d.current_attributes();
            ok(d.execute(create_polyline(p, attributes)));
        });
        assert!(polyline > 10);

        let circle = line_count(&mut builder, &options, |d| {
            let attributes = d.current_attributes();
            ok(d.execute(create_circle(Point2::new(0.0, 0.0), 10.0, attributes)));
        });
        assert!(circle > 10);

        let arc = line_count(&mut builder, &options, |d| {
            let attributes = d.current_attributes();
            ok(d.execute(create_arc(
                Arc2 {
                    center: Point2::new(0.0, 0.0),
                    radius: 10.0,
                    start_angle: 0.0,
                    sweep: 1.5,
                },
                attributes,
            )));
        });
        assert!(arc > 5);
    }

    #[test]
    fn turning_linetypes_off_draws_everything_solid() {
        let mut document = Document::default();
        use_dashed_layer(&mut document);
        line(&mut document, Point2::new(0.0, 0.0), Point2::new(20.0, 0.0));

        let mut builder = SceneBuilder::new();
        let mut dashed = DrawList::default();
        let mut options = plain_options();
        builder.build(&document, &[], &options, &mut dashed, &[]);

        options.draw_linetypes = false;
        let mut solid = DrawList::default();
        builder.build(&document, &[], &options, &mut solid, &[]);

        assert!(dashed.lines.len() > 5);
        assert_eq!(
            solid.lines.len(),
            1,
            "off must mean one line, not a shorter pattern"
        );
    }

    #[test]
    fn a_dashed_path_past_the_budget_is_drawn_whole_and_solid() {
        // All or nothing. A truncating implementation would emit the budget's
        // worth of dashes and stop, silently drawing a 10 km boundary as a
        // 1 km one.
        let mut document = Document::default();
        use_dashed_layer(&mut document);
        line(
            &mut document,
            Point2::new(0.0, 0.0),
            Point2::new(100000.0, 0.0),
        );

        let options = SceneOptions {
            maximum_dash_spans: 100,
            ..plain_options()
        };
        let mut builder = SceneBuilder::new();
        let mut list = DrawList::default();
        builder.build(&document, &[], &options, &mut list, &[]);

        assert_eq!(list.lines.len(), 1, "the whole path, solid");
        let bounds = list.bounds();
        assert!(!bounds.empty());
        near(bounds.max.x, 100000.0, 1e-6);
    }

    #[test]
    fn an_unknown_linetype_name_draws_solid_rather_than_failing() {
        // Layer::linetype has always been a free string with no table behind
        // it, so every project in the field can name a pattern that does not
        // exist.
        let mut document = Document::default();
        let legacy = Layer {
            name: String::from("legacy"),
            linetype: String::from("no-such-pattern"),
            ..Layer::default()
        };
        ok(document.execute(create_layer(legacy)));
        ok(document.set_current_layer("legacy"));
        line(&mut document, Point2::new(0.0, 0.0), Point2::new(20.0, 0.0));

        let mut builder = SceneBuilder::new();
        let mut list = DrawList::default();
        builder.build(&document, &[], &plain_options(), &mut list, &[]);
        assert_eq!(
            list.lines.len(),
            1,
            "drawn solid, exactly as it drew before linetypes existed"
        );
    }
}

// ---- meshes (PLAN.MD 20.2, slice 4) ------------------------------------------------

mod scene_meshes {
    use super::*;

    fn bare() -> SceneOptions<'static> {
        SceneOptions {
            draw_grid: false,
            draw_entities: false,
            ..SceneOptions::default()
        }
    }

    #[test]
    fn a_shaded_mesh_gives_the_renderer_one_triangle_per_face() {
        let mesh = tetrahedron();
        let item = SceneMesh {
            name: String::from("pit"),
            ..mesh_of(&mesh, SurfaceStyle::Shaded)
        };
        let mut out = DrawList::default();
        let mut builder = SceneBuilder::new();
        builder.append_meshes(&[item], &bare(), &mut out);
        assert_eq!(out.triangles.len(), 4);
        assert!(out.lines.is_empty(), "Shaded draws no edges");
    }

    #[test]
    fn every_edge_of_every_face_is_drawn_because_a_mesh_has_no_neighbour_table() {
        // A surface halves its edge count by drawing each shared edge once,
        // which it can because a TIN knows its neighbours. A mesh does not,
        // so all three edges of all four faces are drawn: twelve lines, not
        // six.
        let mesh = tetrahedron();
        let mut out = DrawList::default();
        let mut builder = SceneBuilder::new();
        builder.append_meshes(
            &[mesh_of(&mesh, SurfaceStyle::Wireframe)],
            &bare(),
            &mut out,
        );
        assert_eq!(out.lines.len(), 12);
        assert!(out.triangles.is_empty(), "Wireframe fills nothing");
    }

    #[test]
    fn a_per_face_colour_is_used_and_a_missing_one_falls_back_to_the_mesh_colour() {
        let mesh = tetrahedron();
        // Two of the four faces coloured: the other two take the mesh's
        // colour, so a partly coloured mesh is still wholly drawn.
        let item = SceneMesh {
            flat_color: rgba(10, 20, 30),
            face_colors: vec![rgba(200, 0, 0), rgba(0, 200, 0)],
            ..mesh_of(&mesh, SurfaceStyle::Shaded)
        };
        let options = SceneOptions {
            light_direction: Vec3::new(0.0, 0.0, 0.0), // unlit: colours as given
            ..bare()
        };
        let mut out = DrawList::default();
        let mut builder = SceneBuilder::new();
        builder.append_meshes(&[item], &options, &mut out);
        assert_eq!(out.triangles.len(), 4);
        assert_eq!(out.colors[out.triangles[0].a as usize], rgba(200, 0, 0));
        assert_eq!(out.colors[out.triangles[1].a as usize], rgba(0, 200, 0));
        assert_eq!(out.colors[out.triangles[2].a as usize], rgba(10, 20, 30));
        assert_eq!(out.colors[out.triangles[3].a as usize], rgba(10, 20, 30));
    }

    #[test]
    fn a_mesh_is_framed_and_exaggerated_like_a_surface() {
        let mesh = tetrahedron();
        let item = SceneMesh {
            mesh: Some(&mesh),
            ..SceneMesh::default()
        };
        let document = Document::default();
        let mut options = SceneOptions {
            draw_entities: false,
            ..SceneOptions::default()
        };

        let plain = scene_bounds(&document, &[], &options, std::slice::from_ref(&item));
        assert_eq!(plain.min, Vec3::new(0.0, 0.0, 0.0));
        assert_eq!(plain.max, Vec3::new(10.0, 10.0, 6.0));

        // Exaggeration is applied as the scene is built, so the framing
        // agrees with what is drawn - the same rule surfaces follow.
        options.vertical_exaggeration = 3.0;
        let tall = scene_bounds(&document, &[], &options, std::slice::from_ref(&item));
        assert_eq!(tall.max.z, 18.0);

        let hidden = SceneMesh {
            visible: false,
            ..item
        };
        assert!(
            scene_bounds(&document, &[], &options, &[hidden]).empty(),
            "what is not drawn is not framed"
        );
    }

    #[test]
    fn a_face_naming_a_vertex_that_does_not_exist_is_skipped_rather_than_read() {
        // The scene builder is the last line of defence: a mesh that never
        // went through validate() must not read past its vertices.
        let mut mesh = tetrahedron();
        mesh.faces.push([0, 1, 99]);
        let mut out = DrawList::default();
        let mut builder = SceneBuilder::new();
        builder.append_meshes(&[mesh_of(&mesh, SurfaceStyle::Shaded)], &bare(), &mut out);
        assert_eq!(out.triangles.len(), 4, "the four sound faces, and no fifth");
    }

    #[test]
    fn a_wall_turned_away_from_the_sun_is_darker_than_one_facing_it() {
        // One vertical wall on the line x = y, drawn twice: wound a, b, c its
        // normal (b - a) x (c - a) = (10, 10, 0) x (0, 0, 5) = (50, -50, 0)
        // faces south-east, away from the default sun (-0.5, 0.5, 0.707);
        // wound a, c, b it faces north-west, towards it. The old abs() lit
        // both alike. From SceneOptions' constants:
        //   hemisphere for a level normal (z = 0): 0.10 + (0.30 - 0.10) * 0.5 = 0.20
        //   facing:  n . sun = (0.5 + 0.5) / sqrt 2 = 0.7071, so 0.20 + 0.80 * 0.7071
        //   away:    n . sun < 0 adds nothing: 0.20
        let mesh = TriangleMesh {
            vertices: vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(10.0, 10.0, 0.0),
                Point3::new(0.0, 0.0, 5.0),
            ],
            faces: vec![[0, 1, 2], [0, 2, 1]],
        };
        let base = rgba(200, 200, 200);
        let item = SceneMesh {
            mesh: Some(&mesh),
            flat_color: base,
            ..SceneMesh::default()
        };
        let mut builder = SceneBuilder::new();
        let mut list = DrawList::default();
        builder.append_meshes(&[item], &plain_options(), &mut list);
        assert_eq!(list.triangles.len(), 2);
        assert_eq!(list.colors[list.triangles[0].a as usize], shade(base, 0.20));
        assert_eq!(
            list.colors[list.triangles[1].a as usize],
            shade(base, 0.20 + 0.80 * 0.5_f64.sqrt())
        );
    }
}

// ---- heights, draping, ramp, light, edges (docs/render.md, "The scene") ----------------

mod scene_linework {
    use super::*;

    #[test]
    fn a_three_d_string_is_drawn_at_its_own_heights_even_over_a_surface() {
        // It was drawn at z = 0 whatever its heights, so a road string
        // surveyed at 30-40 m lay under the surface it was measured on.
        let surface = ramp(); // 0..10 under the string
        let mut document = Document::default();
        ok(document.execute(create_entities(vec![three_d_string()])));
        let item = SceneSurface {
            surface: Some(&surface),
            ..SceneSurface::default()
        };
        let layers = layers_of(&document, &[item], &plain_options());
        assert_eq!(layers.entities.positions.len(), 2);
        assert_eq!(layers.entities.positions[0].z, 30.0);
        assert_eq!(layers.entities.positions[1].z, 40.0);
    }

    #[test]
    fn a_plan_line_is_draped_on_the_surface_and_bent_where_it_crosses_an_edge() {
        // ramp() is z = x / 10 over two triangles split along the diagonal
        // from (0, 0) to (100, 100). The line (10, 50)-(90, 50) starts in the
        // upper triangle at z 1, crosses the diagonal at (50, 50) where z is
        // 5, and ends in the lower one at z 9: three vertices, two lines.
        // Without the crossing a straight chord would cut through a ridge or
        // float over a valley of a real TIN.
        let surface = ramp();
        let mut document = Document::default();
        ok(document.execute(create_line(
            Point2::new(10.0, 50.0),
            Point2::new(90.0, 50.0),
            EntityAttributes::default(),
        )));
        let item = SceneSurface {
            surface: Some(&surface),
            ..SceneSurface::default()
        };
        let layers = layers_of(&document, &[item], &plain_options());
        assert_eq!(layers.entities.positions.len(), 3);
        assert_eq!(layers.entities.lines.len(), 2);
        near(layers.entities.positions[0].z, 1.0, 1e-9);
        near(layers.entities.positions[1].x, 50.0, 1e-9);
        near(layers.entities.positions[1].z, 5.0, 1e-9);
        near(layers.entities.positions[2].z, 9.0, 1e-9);
    }

    #[test]
    fn linework_off_every_surface_goes_on_the_datum_of_the_lowest_one() {
        // Two surfaces at 250 and 270; a line nowhere over either has nothing
        // to drape on and no heights, so it goes on the datum: the lower, 250
        // - not on z = 0, 250 m under everything.
        let low = flat_at(0.0, 0.0, 250.0);
        let high = flat_at(200.0, 0.0, 270.0);
        let mut document = Document::default();
        ok(document.execute(create_line(
            Point2::new(-500.0, -500.0),
            Point2::new(-400.0, -450.0),
            EntityAttributes::default(),
        )));
        let a = SceneSurface {
            surface: Some(&low),
            ..SceneSurface::default()
        };
        let b = SceneSurface {
            surface: Some(&high),
            ..SceneSurface::default()
        };
        let layers = layers_of(&document, &[a, b], &plain_options());
        assert_eq!(layers.entities.positions.len(), 2);
        assert_eq!(layers.entities.positions[0].z, 250.0);
        assert_eq!(layers.entities.positions[1].z, 250.0);
        assert_eq!(layers.datum, 250.0);
    }

    #[test]
    fn with_no_surface_linework_without_heights_goes_at_the_lowest_height_drawn() {
        // No terrain: the datum is the lowest height any entity carries, 30
        // (three_d_string), so a plan line sits with the survey rather than
        // 30 m below it at 0.
        let mut document = Document::default();
        ok(document.execute(create_entities(vec![three_d_string()])));
        ok(document.execute(create_line(
            Point2::new(0.0, 0.0),
            Point2::new(10.0, 0.0),
            EntityAttributes::default(),
        )));
        let layers = layers_of(&document, &[], &plain_options());
        assert_eq!(layers.entities.positions.len(), 4);
        assert_eq!(layers.entities.positions[2].z, 30.0);
        assert_eq!(layers.entities.positions[3].z, 30.0);
    }

    #[test]
    fn the_mode_decides_between_heights_drape_and_datum() {
        // The 3D string (heights 30, 40) over ramp(): z = x / 10, so 0 under
        // its start at x 0 and 10 under its end at x 100.
        let surface = ramp();
        let mut document = Document::default();
        ok(document.execute(create_entities(vec![three_d_string()])));
        let item = SceneSurface {
            surface: Some(&surface),
            ..SceneSurface::default()
        };
        let mut options = plain_options();

        options.linework = LineworkHeights::Drape; // heights ignored: on the surface
        let layers = layers_of(&document, std::slice::from_ref(&item), &options);
        assert!(layers.entities.positions.len() >= 2);
        near(layers.entities.positions[0].z, 0.0, 1e-9);
        near(
            layers.entities.positions[layers.entities.positions.len() - 1].z,
            10.0,
            1e-9,
        );

        options.linework = LineworkHeights::Datum; // flat on the lowest surface, 0
        let layers = layers_of(&document, std::slice::from_ref(&item), &options);
        assert_eq!(layers.entities.positions.len(), 2);
        assert_eq!(layers.entities.positions[0].z, 0.0);
        assert_eq!(layers.entities.positions[1].z, 0.0);

        options.linework = LineworkHeights::Heights;
        let layers = layers_of(&document, std::slice::from_ref(&item), &options);
        assert_eq!(layers.entities.positions.len(), 2);
        assert_eq!(layers.entities.positions[0].z, 30.0);
        assert_eq!(layers.entities.positions[1].z, 40.0);
    }

    #[test]
    fn scene_bounds_reach_the_heights_the_linework_is_drawn_at() {
        // Framing uses scene_bounds; it must see the string at 30..40 above a
        // surface of 0..10, or zoom-extents cuts the string's top off.
        let surface = ramp();
        let mut document = Document::default();
        ok(document.execute(create_entities(vec![three_d_string()])));
        let item = SceneSurface {
            surface: Some(&surface),
            ..SceneSurface::default()
        };
        let bounds = scene_bounds(
            &document,
            std::slice::from_ref(&item),
            &plain_options(),
            &[],
        );
        assert_eq!(bounds.min.z, 0.0);
        assert_eq!(bounds.max.z, 40.0);
        // A 3D LINE - a Segment2 with two heights - is read by vertex as well.
        let mut lines = Document::default();
        let mut segment = Entity {
            geometry: Geometry::Line(Segment2::new(Point2::new(0.0, 0.0), Point2::new(10.0, 0.0))),
            ..Entity::default()
        };
        set_heights(&mut segment.properties, &[Some(12.0), Some(55.0)]);
        ok(lines.execute(create_entities(vec![segment])));
        let line_box = scene_bounds(&lines, &[item], &plain_options(), &[]);
        assert_eq!(line_box.max.z, 55.0);
    }
}

mod scene_surfaces {
    use super::*;

    #[test]
    fn one_elevation_ramp_colours_every_visible_surface_alike() {
        // Two surfaces, 0 to 10 high and 10 to 20 high: one ramp from 0 to
        // 20, so the height 10 both have is ONE colour, the ramp's middle, and
        // each surface is not its own blue-to-white. Light off, so the colours
        // are the ramp's own.
        let low = tin(
            vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(100.0, 0.0, 10.0),
                Point3::new(100.0, 100.0, 10.0),
                Point3::new(0.0, 100.0, 0.0),
            ],
            vec![[0, 1, 2], [0, 2, 3]],
        );
        let high = tin(
            vec![
                Point3::new(0.0, 200.0, 10.0),
                Point3::new(100.0, 200.0, 20.0),
                Point3::new(100.0, 300.0, 20.0),
                Point3::new(0.0, 300.0, 10.0),
            ],
            vec![[0, 1, 2], [0, 2, 3]],
        );
        let a = with_surface(&low, SurfaceStyle::Shaded);
        let b = with_surface(&high, SurfaceStyle::Shaded);
        let options = SceneOptions {
            light_direction: Vec3::new(0.0, 0.0, 0.0), // off
            ..plain_options()
        };
        let mut builder = SceneBuilder::new();
        let mut list = DrawList::default();
        builder.append_surfaces(&[a.clone(), b.clone()], &options, &mut list);
        assert_eq!(list.positions.len(), 8);
        let colour_at = |y: f64, z: f64| -> Rgba {
            match list
                .positions
                .iter()
                .position(|position| position.y == y && position.z == z)
            {
                Some(i) => list.colors[i],
                None => panic!("no vertex at y {y} z {z}"),
            }
        };
        assert_eq!(colour_at(0.0, 0.0), elevation_ramp_color(0.0));
        assert_eq!(colour_at(0.0, 10.0), elevation_ramp_color(0.5));
        assert_eq!(colour_at(200.0, 10.0), elevation_ramp_color(0.5));
        assert_eq!(colour_at(200.0, 20.0), elevation_ramp_color(1.0));

        let mut layers = SceneLayers::default();
        builder.build_terrain(&[a, b], &[], &options, &mut layers);
        assert_eq!(layers.ramp_low, 0.0);
        assert_eq!(layers.ramp_high, 20.0);
    }

    #[test]
    fn a_dense_surface_is_drawn_without_edges_by_default_and_a_sparse_one_with_them() {
        // Automatic: edges up to DENSE_SURFACE_TRIANGLES, none above.
        // 160 x 160 vertices make 2 * 159 * 159 = 50 562 triangles, past the
        // 50 000.
        const N: u32 = 160;
        let mut vertices = Vec::new();
        let mut triangles = Vec::new();
        for j in 0..N {
            for i in 0..N {
                vertices.push(Point3::new(
                    f64::from(i),
                    f64::from(j),
                    0.01 * f64::from((i * 7 + j * 3) % 11),
                ));
            }
        }
        for j in 0..N - 1 {
            for i in 0..N - 1 {
                let v = j * N + i;
                triangles.push([v, v + 1, v + N + 1]);
                triangles.push([v, v + N + 1, v + N]);
            }
        }
        let dense = tin(vertices, triangles);
        assert!(dense.triangle_count() > DENSE_SURFACE_TRIANGLES);
        let sparse = ramp();
        let document = Document::default();

        // Style Automatic, the default.
        let item = SceneSurface {
            surface: Some(&dense),
            ..SceneSurface::default()
        };
        let layers = layers_of(&document, &[item], &plain_options());
        assert!(layers.edges.lines.is_empty());
        assert_eq!(layers.terrain.triangles.len(), dense.triangle_count());

        let item = SceneSurface {
            surface: Some(&sparse),
            ..SceneSurface::default()
        };
        let layers = layers_of(&document, &[item], &plain_options());
        assert_eq!(
            layers.edges.lines.len(),
            5,
            "3 + 3 - 1 shared: every edge once"
        );
    }

    #[test]
    fn edges_fade_away_as_the_triangles_shrink_on_screen() {
        // ramp(): two triangles of 5000 m2 each, so the typical edge is the
        // side of an equilateral triangle of that area:
        // sqrt(5000 * 4 / sqrt 3) = 107.5 m. Seen at 2 px to that edge (under
        // 4) the edges are gone; at 50 px (over 12) they are at full strength,
        // the ink colour.
        let surface = ramp();
        let item = with_surface(&surface, SurfaceStyle::ShadedWithEdges);
        let document = Document::default();
        let mut layers = layers_of(&document, &[item], &plain_options());
        assert_eq!(layers.edge_runs.len(), 1);
        near(
            layers.edge_runs[0].typical_edge,
            (5000.0 * 4.0 / 3.0_f64.sqrt()).sqrt(),
            1e-9,
        );

        let mut camera = Camera::default();
        camera.set_viewport_size(500, 500);
        camera.set_projection(Projection::Orthographic);
        let edge = layers.edge_runs[0].typical_edge;
        camera.set_orthographic_height(edge / 2.0 * 500.0); // 2 px per edge
        assert!(!SceneBuilder::fade_edges(&mut layers, &camera));
        assert_eq!(layers.edges.colors, layers.edge_base);
        camera.set_orthographic_height(edge / 50.0 * 500.0); // 50 px per edge
        assert!(SceneBuilder::fade_edges(&mut layers, &camera));
        assert_eq!(layers.edges.colors, layers.edge_ink);
    }
}

/// A line on "design" and one on "asbuilt", the design one selected; the view
/// hides "design" of its own.
struct GhostScene {
    document: Document,
    view: LayerOverrides,
}

impl GhostScene {
    fn new() -> Self {
        let mut document = Document::default();
        for name in ["design", "asbuilt"] {
            ok(document.execute(create_layer(layer_named(name))));
        }
        let design = EntityAttributes {
            layer: String::from("design"),
            ..EntityAttributes::default()
        };
        ok(document.execute(create_line(
            Point2::new(0.0, 0.0),
            Point2::new(10.0, 0.0),
            design,
        )));
        let as_built = EntityAttributes {
            layer: String::from("asbuilt"),
            ..EntityAttributes::default()
        };
        ok(document.execute(create_line(
            Point2::new(0.0, 5.0),
            Point2::new(10.0, 5.0),
            as_built,
        )));
        let first = document.model().entities.ids()[0];
        document.selection_mut().add(first);
        let mut view = LayerOverrides::new();
        assert!(view.hide("design"));
        Self { document, view }
    }

    /// The options the view builds with: its own hides, ghosts on.
    fn options(&self) -> SceneOptions<'_> {
        SceneOptions {
            layers: Some(&self.view),
            selection_ghosts: true,
            ..plain_options()
        }
    }
}

mod scene_layers_build {
    use super::*;

    #[test]
    fn the_selection_overlay_holds_only_the_selected_entities() {
        let mut document = Document::default();
        line(&mut document, Point2::new(0.0, 0.0), Point2::new(10.0, 0.0));
        line(&mut document, Point2::new(0.0, 5.0), Point2::new(10.0, 5.0));
        let id = last_id(&document);
        document.selection_mut().add(id);
        let options = plain_options();
        let layers = layers_of(&document, &[], &options);
        assert_eq!(layers.entities.lines.len(), 2, "the drawing, as drawn");
        assert_eq!(layers.selection.lines.len(), 1);
        assert_eq!(layers.selection.colors[0], options.selection_color);
        assert_eq!(layers.selection.positions[0].y, 5.0);
        assert!(
            layers.selection.lines[0].depth_bias > layers.entities.lines[0].depth_bias,
            "the overlay must win the tie with the entity's own first drawing"
        );
    }

    #[test]
    fn the_selection_overlay_is_the_selection_colour_over_a_dark_casing() {
        // selection_style.rs: a 3 px core in #FF9F1C over a 5 px casing of
        // (16, 18, 22), in a list of its own. The casing's bias is between the
        // core's and the drawing's, so it is not lost behind the line it
        // frames.
        let mut document = Document::default();
        line(&mut document, Point2::new(0.0, 0.0), Point2::new(10.0, 0.0));
        let id = last_id(&document);
        document.selection_mut().add(id);
        let layers = layers_of(&document, &[], &plain_options());
        assert_eq!(layers.selection.lines.len(), 1);
        assert_eq!(layers.selection_casing.lines.len(), 1);
        let core = layers.selection.lines[0];
        let casing = layers.selection_casing.lines[0];
        assert_eq!(
            layers.selection.colors[core.a as usize],
            rgba(0xFF, 0x9F, 0x1C)
        );
        assert_eq!(
            layers.selection.colors[core.b as usize],
            rgba(0xFF, 0x9F, 0x1C)
        );
        assert_eq!(
            layers.selection_casing.colors[casing.a as usize],
            rgba(16, 18, 22)
        );
        assert_eq!(
            layers.selection_casing.colors[casing.b as usize],
            rgba(16, 18, 22)
        );
        assert_eq!(core.width, 3.0);
        assert_eq!(casing.width, 5.0);
        assert!(core.depth_bias > casing.depth_bias);
        assert!(casing.depth_bias > layers.entities.lines[0].depth_bias);
        // The same span.
        assert_eq!(
            layers.selection_casing.positions[casing.a as usize],
            layers.selection.positions[core.a as usize]
        );
        assert_eq!(
            layers.selection_casing.positions[casing.b as usize],
            layers.selection.positions[core.b as usize]
        );
    }

    #[test]
    fn a_selected_point_is_drawn_larger_than_an_unselected_one() {
        // An unselected point is SceneOptions::point_size, 5 px; a selected
        // one a 9 px core over an 11 px casing (selection_style.rs).
        let mut document = Document::default();
        ok(document.execute(create_point(
            Point2::new(3.0, 4.0),
            EntityAttributes::default(),
        )));
        let id = last_id(&document);
        document.selection_mut().add(id);
        let layers = layers_of(&document, &[], &plain_options());
        assert_eq!(layers.entities.points.len(), 1);
        assert_eq!(layers.entities.points[0].size, 5.0);
        assert_eq!(layers.selection.points.len(), 1);
        assert_eq!(layers.selection.points[0].size, 9.0);
        assert_eq!(
            layers.selection.colors[layers.selection.points[0].a as usize],
            rgba(0xFF, 0x9F, 0x1C)
        );
        assert_eq!(layers.selection_casing.points.len(), 1);
        assert_eq!(layers.selection_casing.points[0].size, 11.0);
        assert_eq!(
            layers.selection_casing.colors[layers.selection_casing.points[0].a as usize],
            rgba(16, 18, 22)
        );
    }

    #[test]
    fn a_selected_entity_on_a_layer_the_view_hides_is_one_faint_line_in_the_overlay() {
        // selection_style.rs: one 1 px line of (130, 88, 32) - the selection
        // colour at 45 % over the 3D view's ground - with no casing.
        let scene = GhostScene::new();
        let layers = layers_of(&scene.document, &[], &scene.options());
        assert_eq!(
            layers.entities.lines.len(),
            1,
            "the drawing: the as-built line alone"
        );
        assert_eq!(
            layers.entities.positions[layers.entities.lines[0].a as usize].y,
            5.0
        );
        assert_eq!(layers.selection.lines.len(), 1);
        assert!(
            layers.selection_casing.lines.is_empty(),
            "a ghost has no casing"
        );
        let ghost = layers.selection.lines[0];
        assert_eq!(layers.selection.colors[ghost.a as usize], rgba(130, 88, 32));
        assert_eq!(ghost.width, 1.0);
        assert_eq!(
            layers.selection.positions[ghost.a as usize].y, 0.0,
            "the design line"
        );
        assert_eq!(layers.selection.positions[ghost.b as usize].y, 0.0);
    }

    #[test]
    fn no_ghost_where_the_document_hides_the_layer_or_the_view_turned_ghosts_off() {
        {
            let scene = GhostScene::new();
            let options = SceneOptions {
                selection_ghosts: false,
                ..scene.options()
            };
            let layers = layers_of(&scene.document, &[], &options);
            assert!(layers.selection.lines.is_empty(), "ghosts off");
        }
        {
            let mut scene = GhostScene::new();
            let mut design = scene
                .document
                .model()
                .layers
                .find("design")
                .expect("the design layer")
                .clone();
            design.visible = false;
            ok(scene.document.execute(update_layer(design)));
            let layers = layers_of(&scene.document, &[], &scene.options());
            assert!(
                layers.selection.lines.is_empty(),
                "a layer the drawing hides stays hidden in every view"
            );
        }
        {
            // No view: the document rule alone, which draws it, selected.
            let scene = GhostScene::new();
            let options = SceneOptions {
                layers: None,
                ..scene.options()
            };
            let layers = layers_of(&scene.document, &[], &options);
            assert_eq!(layers.selection.lines.len(), 1);
            assert_eq!(
                layers.selection.colors[layers.selection.lines[0].a as usize],
                options.selection_color
            );
            assert_eq!(layers.selection_casing.lines.len(), 1);
        }
    }

    #[test]
    fn overlay_bounds_are_where_the_overlay_would_draw_the_ids() {
        // What a 3D view frames for ZOOM on a scope
        // (SceneBuilder::overlay_bounds): the ids where build_selection would
        // draw them were they the selection. By hand: the as-built line
        // (0, 5)-(10, 5), and while the view ghosts what it hides, the design
        // line (0, 0)-(10, 0) too - both on the datum a drawing with no
        // heights and no surface stands on, entity_elevation 0.
        let scene = GhostScene::new();
        let ids = scene.document.model().entities.ids();
        assert_eq!(ids.len(), 2); // design, then as-built
        let mut builder = SceneBuilder::new();
        let options = scene.options();
        let layers = layers_of(&scene.document, &[], &options);
        assert_eq!(layers.datum, 0.0);

        let both = builder.overlay_bounds(&scene.document, &[], &options, &layers, &ids);
        assert_eq!(both.min.x, 0.0);
        assert_eq!(both.min.y, 0.0);
        assert_eq!(both.max.x, 10.0);
        assert_eq!(both.max.y, 5.0);
        assert_eq!(both.min.z, 0.0);
        assert_eq!(both.max.z, 0.0);

        // Ghosts off, the design line is nowhere this view draws: the
        // as-built line alone, and nothing for the design line alone.
        let options = SceneOptions {
            selection_ghosts: false,
            ..options
        };
        let shown = builder.overlay_bounds(&scene.document, &[], &options, &layers, &ids);
        assert_eq!(shown.min.y, 5.0);
        assert_eq!(shown.max.y, 5.0);
        let design_only = [ids[0]];
        assert!(
            builder
                .overlay_bounds(&scene.document, &[], &options, &layers, &design_only)
                .empty()
        );
        assert!(
            builder
                .overlay_bounds(&scene.document, &[], &options, &layers, &[])
                .empty()
        );
        // The ids asked about are framed, not the selection: the as-built
        // line is not selected.
        assert!(!scene.document.selection().contains(ids[1]));
    }

    #[test]
    fn the_overlay_walking_the_selection_finds_exactly_what_a_walk_of_the_drawing_finds() {
        // CLAUDE.md section 5: the overlay was culled from a walk of the whole
        // drawing, keeping the selected, to a walk of the selection's ids
        // alone (BM_SceneSelectionBuild, docs/render.md). The one-list build
        // still walks the drawing, and draws each selected entity it meets in
        // the selection colour: its selection-coloured lines and points, in
        // order, are what the exhaustive walk kept. The overlay is exactly
        // those - the same vertices, in the same order - with unselected
        // entities between the selected ones, a selected entity on a layer the
        // drawing hides, and a selected id whose entity has been deleted.
        let mut document = Document::default();
        let off = Layer {
            name: String::from("off"),
            visible: false,
            ..Layer::default()
        };
        ok(document.execute(create_layer(off)));
        let hidden = EntityAttributes {
            layer: String::from("off"),
            ..EntityAttributes::default()
        };
        let default = EntityAttributes::default;
        let mut selected = Vec::new();
        ok(document.execute(create_line(
            Point2::new(0.0, 0.0),
            Point2::new(10.0, 0.0),
            default(),
        )));
        selected.push(last_id(&document));
        ok(document.execute(create_line(
            Point2::new(0.0, 5.0),
            Point2::new(10.0, 5.0),
            default(),
        )));
        ok(document.execute(create_polyline(
            Polyline2::new(
                vec![
                    Point2::new(20.0, 0.0),
                    Point2::new(30.0, 0.0),
                    Point2::new(30.0, 10.0),
                    Point2::new(20.0, 10.0),
                ],
                true,
            ),
            default(),
        )));
        selected.push(last_id(&document));
        ok(document.execute(create_circle(Point2::new(50.0, 50.0), 5.0, default())));
        selected.push(last_id(&document));
        ok(document.execute(create_point(Point2::new(60.0, 60.0), default())));
        ok(document.execute(create_point(Point2::new(70.0, 70.0), default())));
        selected.push(last_id(&document));
        ok(document.execute(create_line(
            Point2::new(0.0, 20.0),
            Point2::new(5.0, 25.0),
            hidden,
        )));
        selected.push(last_id(&document));
        ok(document.execute(create_line(
            Point2::new(80.0, 0.0),
            Point2::new(90.0, 0.0),
            default(),
        )));
        let gone = last_id(&document);
        ok(document.execute(delete_entities(vec![gone])));
        for &id in &selected {
            document.selection_mut().add(id);
        }
        document.selection_mut().add(gone); // an id the drawing no longer holds

        let options = plain_options();
        let mut builder = SceneBuilder::new();
        let mut walked = DrawList::default();
        builder.build(&document, &[], &options, &mut walked, &[]);
        let layers = layers_of(&document, &[], &options);
        let culled = &layers.selection;

        type Ends = [f64; 6];
        let ends_of = |list: &DrawList, a: u32, b: u32| -> Ends {
            let p = list.positions[a as usize];
            let q = list.positions[b as usize];
            [p.x, p.y, p.z, q.x, q.y, q.z]
        };
        let walked_lines: Vec<Ends> = walked
            .lines
            .iter()
            .filter(|line| walked.colors[line.a as usize] == options.selection_color)
            .map(|line| ends_of(&walked, line.a, line.b))
            .collect();
        let walked_points: Vec<Ends> = walked
            .points
            .iter()
            .filter(|point| walked.colors[point.a as usize] == options.selection_color)
            .map(|point| ends_of(&walked, point.a, point.a))
            .collect();
        let culled_lines: Vec<Ends> = culled
            .lines
            .iter()
            .map(|line| ends_of(culled, line.a, line.b))
            .collect();
        let culled_points: Vec<Ends> = culled
            .points
            .iter()
            .map(|point| ends_of(culled, point.a, point.a))
            .collect();
        // It reaches what it claims to: the line, the closed polyline's four
        // sides, the circle's chords and the point - and nothing of the hidden
        // line or of the deleted one.
        assert!(walked_lines.len() > 1 + 4 + 8);
        assert_eq!(walked_points.len(), 1);
        assert_eq!(culled_lines, walked_lines);
        assert_eq!(culled_points, walked_points);
    }
}

// ---- one frame: the layers drawn as the 3D view draws them -----------------------

const BLACK: Rgba = rgba(0, 0, 0);

/// A TIN of n x n square cells over the square `size` wide from (x0, y0), at
/// the heights `height` gives.
fn grid_tin(x0: f64, y0: f64, size: f64, n: i32, height: impl Fn(f64, f64) -> f64) -> TinSurface {
    let mut vertices = Vec::new();
    for j in 0..=n {
        for i in 0..=n {
            let x = x0 + size * f64::from(i) / f64::from(n);
            let y = y0 + size * f64::from(j) / f64::from(n);
            vertices.push(Point3::new(x, y, height(x, y)));
        }
    }
    let mut triangles = Vec::new();
    for j in 0..n {
        for i in 0..n {
            let v0 = (j * (n + 1) + i) as u32;
            let v3 = v0 + (n + 1) as u32;
            triangles.push([v0, v0 + 1, v3 + 1]);
            triangles.push([v0, v3 + 1, v3]);
        }
    }
    tin(vertices, triangles)
}

/// One frame of `layers` through a copy of `camera`, on one thread, over
/// black.
fn frame_of(layers: &mut SceneLayers, camera: &Camera) -> Framebuffer {
    let mut target = match Framebuffer::create(camera.viewport_width(), camera.viewport_height()) {
        Ok(target) => target,
        Err(error) => panic!("{}", error.describe()),
    };
    let pool = TaskPool::new(0);
    let options = RenderOptions {
        background: BLACK,
        pool: Some(&pool),
        ..RenderOptions::default()
    };
    let mut rasterizer = Rasterizer::default();
    let mut camera = *camera;
    if let Err(error) = render_layers(layers, &mut camera, &mut rasterizer, &mut target, options) {
        panic!("{}", error.describe());
    }
    target
}

fn count_pixels(frame: &Framebuffer, colour: Rgba) -> usize {
    frame
        .color()
        .iter()
        .filter(|&&pixel| pixel == colour)
        .count()
}

mod scene_frame {
    use super::*;

    #[test]
    fn a_flat_surface_at_the_datum_covers_the_grid_standing_under_it() {
        // A pad flat at 5 m, 200 m square in 13 x 13 cells. The datum is its
        // lowest point, 5, so the grid lies exactly in its plane. A filled
        // triangle is pushed back by its own depth slope, and the grid drawn
        // before it with depth won: 8,902 of the pad's 235,340 pixels at the
        // iso view were grid lines, the whole grid pattern across the pad.
        // Drawn as a backdrop the grid is under the pad everywhere, so every
        // pixel of the pad is what the pad draws with no grid at all.
        let pad = grid_tin(0.0, 0.0, 200.0, 13, |_, _| 5.0);
        let item = with_surface(&pad, SurfaceStyle::Shaded);
        let document = Document::default();
        let with_grid = SceneOptions::default(); // the grid the view draws by default
        let no_grid = SceneOptions {
            draw_grid: false,
            ..with_grid
        };
        let mut gridded = layers_of(&document, std::slice::from_ref(&item), &with_grid);
        let mut bare = layers_of(&document, std::slice::from_ref(&item), &no_grid);
        assert!(!gridded.grid.lines.is_empty());
        near(gridded.grid.bounds().min.z, 5.0, 1e-9);

        let views = [
            (Projection::Perspective, StandardView::IsoSouthWest),
            (Projection::Orthographic, StandardView::IsoSouthWest),
            (Projection::Perspective, StandardView::Top),
            (Projection::Orthographic, StandardView::Top),
        ];
        for (projection, standard) in views {
            let mut camera = Camera::default();
            camera.set_viewport_size(800, 600);
            camera.set_projection(projection);
            camera.set_standard_view(standard);
            assert!(camera.frame(bare.bounds));
            let plain = frame_of(&mut bare, &camera);
            let seen = frame_of(&mut gridded, &camera);
            let mut pad_pixels = 0;
            let mut changed = 0;
            let mut grid_beside = 0;
            for (&before, &after) in plain.color().iter().zip(seen.color()) {
                if before != BLACK {
                    pad_pixels += 1;
                    changed += usize::from(after != before);
                } else if after != BLACK {
                    grid_beside += 1;
                }
            }
            assert!(
                pad_pixels > 50000,
                "{projection:?} {standard:?}: the pad is not in view, so this proves nothing"
            );
            assert_eq!(
                changed, 0,
                "{projection:?} {standard:?}: grid pixels drawn over the pad, of {pad_pixels}"
            );
            assert!(
                grid_beside > 1000,
                "{projection:?} {standard:?}: the grid was not drawn at all"
            );
        }
    }

    #[test]
    fn drawing_lines_draped_on_a_surface_cross_its_edges_unbroken() {
        // A plane rising 5% east and 2% north, 400 m square in 40 x 40 cells:
        // 3,200 triangles, under DENSE_SURFACE_TRIANGLES, so the default style
        // draws its edges, and 10 m cells are 20 px or more at the frame, so
        // at full strength. Across it 40 plan lines, draped. An edge (1
        // footprint) and a draped line (1.5) each have one depth across their
        // width; off their centres those differ by up to a pixel of the
        // plane's slope, which at a low angle is more than the half footprint
        // between them, so the edge drawn first won every crossing: 2.4% of
        // the line pixels lost at the iso view, 9.4% at 0.25 rad, 18.7% at
        // 0.12. Edges that write no depth leave the lines exactly the pixels
        // they draw on the bare plane.
        let plane = grid_tin(0.0, 0.0, 400.0, 40, |x, y| 0.05 * x + 0.02 * y);
        let mut document = Document::default();
        for k in 0..30 {
            let y = 7.0 + 12.0 * f64::from(k);
            line(
                &mut document,
                Point2::new(5.0, y),
                Point2::new(395.0, y + 30.0),
            );
        }
        for k in 0..10 {
            let x = 13.0 + 37.0 * f64::from(k);
            line(
                &mut document,
                Point2::new(x, 5.0),
                Point2::new(x + 20.0, 395.0),
            );
        }
        // The default, Automatic: edges at this size.
        let edged = SceneSurface {
            surface: Some(&plane),
            ..SceneSurface::default()
        };
        let shaded = SceneSurface {
            style: SurfaceStyle::Shaded,
            ..edged.clone()
        };
        let mut with_edges = layers_of(&document, &[edged], &plain_options());
        let mut bare = layers_of(&document, &[shaded], &plain_options());
        assert!(
            !with_edges.edges.lines.is_empty(),
            "no edges, so this proves nothing"
        );
        assert!(bare.edges.lines.is_empty());
        let ink = bare.entities.colors[0];

        for projection in [Projection::Perspective, Projection::Orthographic] {
            for elevation in [0.61, 0.25, 0.12] {
                let mut camera = Camera::default();
                camera.set_viewport_size(1200, 800);
                camera.set_projection(projection);
                camera.set_standard_view(StandardView::IsoSouthWest);
                camera.set_orientation(camera.azimuth(), elevation);
                assert!(camera.frame(bare.bounds));
                let plain = frame_of(&mut bare, &camera);
                let alone = count_pixels(&plain, ink);
                assert!(
                    alone > 5000,
                    "{projection:?} at {elevation}: the lines are not in view, so this proves nothing"
                );
                let seen = frame_of(&mut with_edges, &camera);
                assert_eq!(
                    count_pixels(&seen, ink),
                    alone,
                    "{projection:?} at {elevation}"
                );
                let edge_pixels = plain
                    .color()
                    .iter()
                    .zip(seen.color())
                    .filter(|(before, after)| before != after)
                    .count();
                assert!(
                    edge_pixels > 5000,
                    "{projection:?} at {elevation}: the edges were not drawn, so this proves nothing"
                );
            }
        }
    }

    #[test]
    fn of_two_coincident_surfaces_the_edges_shown_are_those_of_the_surface_shown() {
        // A design surface often repeats the existing one's triangles outside
        // the works. Coincident, the surface drawn first wins the tie and is
        // the one seen; its edges won too while edges wrote depth. Writing
        // none, the second surface's edges - drawn later, and in a real
        // archive fainter, its triangles being smaller on average - painted
        // over the edges of the surface seen. The edges are emitted surface by
        // surface in reverse, so painted in order the first surface's are on
        // top. Here the two differ only in their flat colour, so the second's
        // edges have their own colours and not one pixel of them may show.
        let ground = grid_tin(0.0, 0.0, 200.0, 10, |x, y| 0.03 * x + 0.01 * y);
        let first = SceneSurface {
            coloring: SurfaceColoring::Flat,
            flat_color: rgba(90, 150, 90),
            ..with_surface(&ground, SurfaceStyle::ShadedWithEdges)
        };
        let second = SceneSurface {
            flat_color: rgba(150, 90, 150),
            ..first.clone()
        };
        let document = Document::default();
        let mut layers = layers_of(&document, &[first, second], &plain_options());
        assert_eq!(layers.edge_runs.len(), 2);

        let mut camera = Camera::default();
        camera.set_viewport_size(800, 600);
        camera.set_standard_view(StandardView::IsoSouthWest);
        assert!(camera.frame(layers.bounds));
        let seen = frame_of(&mut layers, &camera);
        // The colours each surface's edges were drawn in this frame
        // (fade_edges sets them for the camera).
        let ink_of = |run: usize| -> Vec<Rgba> {
            let run = layers.edge_runs[run];
            let mut inks =
                layers.edges.colors[run.first as usize..(run.first + run.count) as usize].to_vec();
            inks.sort_unstable();
            inks.dedup();
            inks
        };
        let count_of = |inks: &[Rgba]| {
            seen.color()
                .iter()
                .filter(|colour| inks.binary_search(colour).is_ok())
                .count()
        };
        let first_ink = ink_of(0);
        let second_ink = ink_of(1);
        for colour in &second_ink {
            assert!(
                first_ink.binary_search(colour).is_err(),
                "the two surfaces share an edge colour, so this proves nothing"
            );
        }
        assert!(
            count_of(&first_ink) > 2000,
            "the first surface's edges were not drawn"
        );
        assert_eq!(count_of(&second_ink), 0);
    }

    #[test]
    fn a_selected_line_is_drawn_in_the_selection_colour_between_two_edges_of_its_casing() {
        // A line along x through the middle of the default iso view, selected:
        // down the middle column the first and last pixels drawn are the
        // casing's (16, 18, 22), with the core's #FF9F1C between them and none
        // of the line's own white - the casing drawn first writes no depth, so
        // the core covers it, and the core's pull beats the line's (scene.rs,
        // render_layers). Not the top view: a flat drawing seen face-on has a
        // depth range thinner than one pixel's pull, every pull stops at the
        // near plane and the line's own drawing wins the tie (docs/render.md,
        // "Outstanding").
        let mut document = Document::default();
        line(
            &mut document,
            Point2::new(0.0, 0.0),
            Point2::new(100.0, 0.0),
        );
        let id = last_id(&document);
        document.selection_mut().add(id);
        let mut layers = layers_of(&document, &[], &plain_options());
        let mut camera = Camera::default();
        camera.set_viewport_size(200, 100);
        camera.set_projection(Projection::Perspective);
        camera.set_standard_view(StandardView::IsoSouthWest);
        assert!(camera.frame(layers.bounds));
        let seen = frame_of(&mut layers, &camera);
        const CORE: Rgba = rgba(0xFF, 0x9F, 0x1C);
        const CASING: Rgba = rgba(16, 18, 22);
        let column: Vec<Rgba> = (0..100)
            .map(|row| seen.color_at(100, row))
            .filter(|&colour| colour != BLACK)
            .collect();
        let drawn: String = column
            .iter()
            .map(|colour| format!(" {:06x}", colour & 0x00FF_FFFF))
            .collect();
        assert!(
            column.len() >= 5,
            "a 5 px casing is at least 5 rows:{drawn}"
        );
        assert_eq!(column[0], CASING, "{drawn}");
        assert_eq!(column[column.len() - 1], CASING, "{drawn}");
        assert!(
            column.iter().filter(|&&colour| colour == CORE).count() >= 2,
            "the 3 px core:{drawn}"
        );
        // Nothing but the two, and the core in one run between the casing's.
        let first = column
            .iter()
            .position(|&colour| colour == CORE)
            .expect("a core");
        let last = column.len()
            - column
                .iter()
                .rev()
                .position(|&colour| colour == CORE)
                .expect("a core");
        assert!(
            column[first..last].iter().all(|&colour| colour == CORE),
            "{drawn}"
        );
        assert!(
            column[..first].iter().all(|&colour| colour == CASING),
            "{drawn}"
        );
        assert!(
            column[last..].iter().all(|&colour| colour == CASING),
            "{drawn}"
        );
    }

    #[test]
    fn the_depth_range_takes_in_a_ghost_outside_what_the_view_draws() {
        // The view draws the as-built line at the origin and hides "design",
        // where the selected line lies a kilometre further along the view
        // direction. Framed on what is drawn, the depth range fitted to that
        // alone ends short of the ghost; with the overlay taken in it reaches
        // it.
        let mut document = Document::default();
        for name in ["design", "asbuilt"] {
            ok(document.execute(create_layer(layer_named(name))));
        }
        let as_built = EntityAttributes {
            layer: String::from("asbuilt"),
            ..EntityAttributes::default()
        };
        ok(document.execute(create_line(
            Point2::new(0.0, 0.0),
            Point2::new(10.0, 0.0),
            as_built,
        )));
        let design = EntityAttributes {
            layer: String::from("design"),
            ..EntityAttributes::default()
        };
        ok(document.execute(create_line(
            Point2::new(1000.0, 1000.0),
            Point2::new(1010.0, 1000.0),
            design,
        )));
        let id = last_id(&document);
        document.selection_mut().add(id);
        let mut view = LayerOverrides::new();
        assert!(view.hide("design"));

        let depths_of_the_ghost = |camera: &Camera| {
            let f = camera.forward();
            let e = camera.eye();
            (
                (Vec3::new(1000.0, 1000.0, 0.0) - e).dot(f),
                (Vec3::new(1010.0, 1000.0, 0.0) - e).dot(f),
            )
        };
        let fitted = |ghosts: bool| -> Camera {
            let options = SceneOptions {
                layers: Some(&view),
                selection_ghosts: ghosts,
                ..plain_options()
            };
            let mut layers = layers_of(&document, &[], &options);
            let mut camera = Camera::default();
            camera.set_viewport_size(400, 300);
            camera.set_standard_view(StandardView::IsoSouthWest);
            assert!(camera.frame(layers.bounds));
            let mut target = Framebuffer::create(400, 300).expect("a target");
            let pool = TaskPool::new(0);
            let render = RenderOptions {
                pool: Some(&pool),
                ..RenderOptions::default()
            };
            let mut rasterizer = Rasterizer::default();
            if let Err(error) = render_layers(
                &mut layers,
                &mut camera,
                &mut rasterizer,
                &mut target,
                render,
            ) {
                panic!("{}", error.describe());
            }
            camera
        };

        let without = fitted(false);
        let (near_without, far_without) = depths_of_the_ghost(&without);
        assert!(
            katana_math::cpp::max(near_without, far_without) > without.far_plane(),
            "the ghost is inside the drawn box's range anyway, so this proves nothing"
        );

        let with = fitted(true);
        let (a, b) = depths_of_the_ghost(&with);
        assert!(katana_math::cpp::max(a, b) <= with.far_plane());
        assert!(katana_math::cpp::min(a, b) >= with.near_plane());
    }
}
