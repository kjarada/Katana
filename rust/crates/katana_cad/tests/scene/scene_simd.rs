// The scene build's kernels (simd/scene_avx2.rs on x86-64, scene_neon.rs on
// 64-bit ARM) against its scalar code: every draw list compared element by
// element, to the bit (tests/cad/test_scene_simd.cpp).
//
// Each test builds the same scene twice, once at each level, in one process.
// Where the C++ skips on a processor with no kernels, these fail instead
// (simd_levels.rs, require_kernels): a comparison of the scalar path with
// itself does not count as having compared the kernels.

use crate::simd_levels::{ScopedSimdLevel, at_simd_level, require_kernels};
use crate::tin;
use katana_cad::scene::{
    EdgeRun, SceneBuilder, SceneLayers, SceneMesh, SceneOptions, SceneSurface, SurfaceColoring,
    apply_surface_display,
};
use katana_cad::terrain::surface_style::{SurfaceDisplayStyle, SurfaceStyle, surface_ramp_names};
use katana_core::cpu_features::SimdLevel;
use katana_geometry::{Point3, TriangleMesh};
use katana_math::{AABB, Vec3};
use katana_render::{
    Camera, DrawLine, DrawList, DrawPoint, DrawTriangle, VertexIndex, rgba, rgba_with_alpha,
};
use katana_terrain::TinSurface;

/// The first element of two lists whose words differ, with both words.
#[track_caller]
fn expect_same_bits<T, W: PartialEq + std::fmt::Debug>(
    scalar: &[T],
    kernel: &[T],
    words: impl Fn(&T) -> W,
    what: &str,
) {
    assert_eq!(scalar.len(), kernel.len(), "{what}");
    for (i, (left, right)) in scalar.iter().zip(kernel).enumerate() {
        let (l, r) = (words(left), words(right));
        assert!(
            l == r,
            "{what} differs first at element {i} of {} (scalar/kernel words: {l:?}/{r:?})",
            scalar.len()
        );
    }
}

fn position_words(p: &Point3) -> [u64; 3] {
    [p.x.to_bits(), p.y.to_bits(), p.z.to_bits()]
}

fn triangle_words(t: &DrawTriangle) -> [u32; 3] {
    [t.a, t.b, t.c]
}

fn line_words(l: &DrawLine) -> [u32; 4] {
    [l.a, l.b, l.width.to_bits(), l.depth_bias.to_bits()]
}

fn point_words(p: &DrawPoint) -> [u32; 3] {
    [p.a, p.size.to_bits(), p.depth_bias.to_bits()]
}

#[track_caller]
fn expect_same_list(scalar: &DrawList, kernel: &DrawList, what: &str) {
    expect_same_bits(
        &scalar.positions,
        &kernel.positions,
        position_words,
        &format!("{what} positions"),
    );
    expect_same_bits(
        &scalar.colors,
        &kernel.colors,
        |&c| c,
        &format!("{what} colours"),
    );
    expect_same_bits(
        &scalar.triangles,
        &kernel.triangles,
        triangle_words,
        &format!("{what} triangles"),
    );
    expect_same_bits(
        &scalar.lines,
        &kernel.lines,
        line_words,
        &format!("{what} lines"),
    );
    expect_same_bits(
        &scalar.points,
        &kernel.points,
        point_words,
        &format!("{what} points"),
    );
}

fn box_words(b: &AABB) -> [u64; 6] {
    [
        b.min.x.to_bits(),
        b.min.y.to_bits(),
        b.min.z.to_bits(),
        b.max.x.to_bits(),
        b.max.y.to_bits(),
        b.max.z.to_bits(),
    ]
}

#[track_caller]
fn expect_same_box(a: &AABB, b: &AABB, what: &str) {
    assert_eq!(box_words(a), box_words(b), "{what}");
}

#[track_caller]
fn expect_same_layers(scalar: &SceneLayers, kernel: &SceneLayers) {
    expect_same_list(&scalar.terrain, &kernel.terrain, "terrain");
    expect_same_list(&scalar.edges, &kernel.edges, "edges");
    expect_same_bits(&scalar.edge_base, &kernel.edge_base, |&c| c, "edge base");
    expect_same_bits(&scalar.edge_ink, &kernel.edge_ink, |&c| c, "edge ink");
    expect_same_box(&scalar.bounds, &kernel.bounds, "bounds");
}

/// A cells x cells grid of squares, two triangles each with the diagonal
/// alternating, over rolling ground at map-grid coordinates. Its vertex
/// count, (cells + 1)^2, is 1, 0 or 1 past a multiple of four, so the
/// kernels' padded last block is exercised as well as whole ones.
fn rolling_tin(cells: i32, east: f64, north: f64) -> TinSurface {
    let side = cells + 1;
    let mut vertices = Vec::new();
    for j in 0..side {
        for i in 0..side {
            let x = 7.0 * f64::from(i);
            let y = 5.0 * f64::from(j);
            vertices.push(Point3::new(
                east + x,
                north + y,
                40.0 + 6.0 * (x * 0.05).sin() * (y * 0.07).cos() + 0.3 * (x * 0.9 + y * 0.4).sin(),
            ));
        }
    }
    let mut triangles = Vec::new();
    for j in 0..cells {
        for i in 0..cells {
            let v0 = (j * side + i) as u32;
            let v1 = v0 + 1;
            let v2 = v0 + side as u32 + 1;
            let v3 = v0 + side as u32;
            if (i + j) % 2 == 0 {
                triangles.push([v0, v1, v2]);
                triangles.push([v0, v2, v3]);
            } else {
                triangles.push([v0, v1, v3]);
                triangles.push([v1, v2, v3]);
            }
        }
    }
    tin(vertices, triangles)
}

fn rolling_at_origin(cells: i32) -> TinSurface {
    rolling_tin(cells, 300000.0, 6250000.0)
}

fn terrain_at(
    level: SimdLevel,
    surfaces: &[SceneSurface<'_>],
    meshes: &[SceneMesh<'_>],
    options: &SceneOptions<'_>,
) -> SceneLayers {
    at_simd_level(level, || {
        let mut builder = SceneBuilder::new();
        let mut layers = SceneLayers::default();
        builder.build_terrain(surfaces, meshes, options, &mut layers);
        layers
    })
}

#[track_caller]
fn expect_same_terrain(
    kernel_level: SimdLevel,
    surfaces: &[SceneSurface<'_>],
    meshes: &[SceneMesh<'_>],
    options: &SceneOptions<'_>,
) {
    let scalar = terrain_at(SimdLevel::Scalar, surfaces, meshes, options);
    let kernel = terrain_at(kernel_level, surfaces, meshes, options);
    expect_same_layers(&scalar, &kernel);
}

fn styled_item(
    surface: &TinSurface,
    style: SurfaceStyle,
    coloring: SurfaceColoring,
) -> SceneSurface<'_> {
    SceneSurface {
        surface: Some(surface),
        style,
        coloring,
        flat_color: rgba_with_alpha(120, 200, 90, 180),
        ..SceneSurface::default()
    }
}

mod scene_kernels {
    use super::*;

    #[test]
    fn a_lit_ramp_coloured_tin_builds_the_same_draw_lists_at_every_level() {
        let kernel = require_kernels();
        // 1 to 9 cells: 4 to 100 vertices, both sides of the dispatch
        // threshold and every remainder of four; then one big enough to be all
        // kernel.
        for cells in [1, 2, 3, 4, 5, 6, 7, 8, 9, 60] {
            let surface = rolling_at_origin(cells);
            let surfaces = [SceneSurface {
                surface: Some(&surface),
                style: SurfaceStyle::Shaded,
                ..SceneSurface::default()
            }];
            let mut options = SceneOptions::default();
            expect_same_terrain(kernel, &surfaces, &[], &options);
            // Exaggerated about a datum, from a low sun in the north-east.
            options.vertical_exaggeration = 2.5;
            options.exaggeration_datum = 37.25;
            options.light_direction = Vec3::new(0.6, 0.7, 0.25);
            expect_same_terrain(kernel, &surfaces, &[], &options);
        }
    }

    #[test]
    fn every_style_and_colouring_of_a_surface_builds_the_same_at_every_level() {
        let kernel = require_kernels();
        let a = rolling_at_origin(11);
        let b = rolling_tin(6, 300020.0, 6250010.0);
        for style in [
            SurfaceStyle::Shaded,
            SurfaceStyle::ShadedWithEdges,
            SurfaceStyle::Wireframe,
            SurfaceStyle::Automatic,
        ] {
            for coloring in [
                SurfaceColoring::Elevation,
                SurfaceColoring::Flat,
                SurfaceColoring::Slope,
            ] {
                let surfaces = [
                    styled_item(&a, style, coloring),
                    styled_item(&b, style, coloring),
                ];
                let mut options = SceneOptions::default();
                expect_same_terrain(kernel, &surfaces, &[], &options);
                options.light_direction = Vec3::new(0.0, 0.0, 0.0); // unlit: no normals at all
                expect_same_terrain(kernel, &surfaces, &[], &options);
            }
        }
    }

    #[test]
    fn a_surface_in_a_display_style_on_every_ramp_builds_the_same_at_every_level() {
        // Surface display (docs/surface_display.md): the kernels' ramp table
        // is filled from the ramp a style names, so a surface on any ramp of
        // the table builds the same at both levels - smooth on the kernel
        // path, banded on the scalar one, which must not reach the kernels at
        // all.
        let kernel = require_kernels();
        let a = rolling_at_origin(11);
        for &ramp in surface_ramp_names() {
            for banded in [false, true] {
                let style = SurfaceDisplayStyle {
                    name: String::from("test"),
                    ramp: String::from(ramp),
                    banded,
                    shading: SurfaceStyle::ShadedWithEdges,
                    ..SurfaceDisplayStyle::default()
                };
                let mut item = SceneSurface {
                    surface: Some(&a),
                    ..SceneSurface::default()
                };
                apply_surface_display(&mut item, &style);
                expect_same_terrain(kernel, &[item], &[], &SceneOptions::default());
            }
        }
    }

    #[test]
    fn a_surface_of_one_elevation_takes_the_ramps_middle_colour_at_every_level() {
        let kernel = require_kernels();
        // A flat TIN spans no elevation, so the ramp is its middle colour,
        // elevation_ramp_color(0.5) = (235, 220, 130), before the light shades
        // it: a flat face lit from straight above takes sky 1.0 and the whole
        // sun.
        let mut vertices = Vec::new();
        let mut triangles = Vec::new();
        for j in 0..5 {
            for i in 0..5 {
                vertices.push(Point3::new(10.0 * f64::from(i), 10.0 * f64::from(j), 12.0));
            }
        }
        for j in 0..4u32 {
            for i in 0..4u32 {
                let v = j * 5 + i;
                triangles.push([v, v + 1, v + 6]);
                triangles.push([v, v + 6, v + 5]);
            }
        }
        let surface = tin(vertices, triangles);
        let surfaces = [SceneSurface {
            surface: Some(&surface),
            style: SurfaceStyle::Shaded,
            ..SceneSurface::default()
        }];
        let options = SceneOptions {
            light_direction: Vec3::new(0.0, 0.0, 1.0),
            sky_ambient: 1.0,
            ground_ambient: 0.0,
            sun_strength: 0.0,
            ..SceneOptions::default()
        };
        let scalar = terrain_at(SimdLevel::Scalar, &surfaces, &[], &options);
        let kernels = terrain_at(kernel, &surfaces, &[], &options);
        expect_same_layers(&scalar, &kernels);
        assert_eq!(kernels.terrain.colors.len(), 25);
        assert_eq!(kernels.terrain.colors[12], rgba(235, 220, 130));
    }

    #[test]
    fn meshes_build_the_same_at_every_level_including_bad_and_degenerate_faces() {
        let kernel = require_kernels();
        // Boxes of 12 faces, cut to every count from 1 to 25 so each remainder
        // of the kernel's four faces is met, with a face naming a vertex that
        // is not there (skipped at both levels), a face of zero area (left
        // unlit) and colours for some faces only.
        const FACES: [[u32; 3]; 12] = [
            [0, 2, 1],
            [1, 2, 3],
            [4, 5, 6],
            [5, 7, 6],
            [0, 1, 4],
            [1, 5, 4],
            [2, 6, 3],
            [3, 6, 7],
            [0, 4, 2],
            [2, 4, 6],
            [1, 3, 5],
            [3, 7, 5],
        ];
        for faces in 1..=25usize {
            let mut mesh = TriangleMesh::default();
            for corner_box in 0..3 {
                let first = mesh.vertices.len() as u32;
                let offset = f64::from(corner_box);
                for k in 0..8 {
                    mesh.vertices.push(Point3::new(
                        300000.0 + 3.0 * offset + if (k & 1) != 0 { 1.1 } else { 0.0 },
                        6250000.0 + if (k & 2) != 0 { 1.7 } else { 0.0 },
                        40.0 + 0.5 * offset + if (k & 4) != 0 { 2.3 } else { 0.0 },
                    ));
                }
                for face in FACES {
                    mesh.faces
                        .push([first + face[0], first + face[1], first + face[2]]);
                }
            }
            mesh.faces[4] = [0, 1, 999]; // names no vertex
            mesh.faces[7] = [2, 2, 3]; // no area
            mesh.faces.truncate(faces);
            let mut second = SceneMesh {
                mesh: Some(&mesh),
                flat_color: rgba_with_alpha(10, 250, 128, 200),
                ..SceneMesh::default()
            };
            for f in 0..faces / 2 {
                // static_cast<std::uint8_t> of 30 f wraps past 255, as `as u8`
                // does.
                second.face_colors.push(rgba((30 * f) as u8, 90, 255));
            }
            let items = [
                SceneMesh {
                    mesh: Some(&mesh),
                    ..SceneMesh::default()
                },
                second,
            ];
            let mut options = SceneOptions {
                vertical_exaggeration: 3.0,
                exaggeration_datum: 40.0,
                ..SceneOptions::default()
            };
            expect_same_terrain(kernel, &[], &items, &options);
            options.light_direction = Vec3::new(0.0, 0.0, 0.0);
            expect_same_terrain(kernel, &[], &items, &options);
        }
    }

    #[test]
    fn normals_too_long_to_square_light_the_same_at_every_level() {
        let kernel = require_kernels();
        // The hypot of a normal whose squares overflow a double: a surface
        // lifted 1e200 times over has face normals near 1e201 (a 7 x 5 m
        // cell, relief of metres), whose squares, near 1e402, are past DBL_MAX
        // (1.8e308). libstdc++ divides by the largest component first, as
        // katana_math's Vec3 length does: the kernels copy it, and this is
        // where it parts from the naive formula. A mesh as large, and one with
        // a NaN corner, too.
        let surface = rolling_at_origin(9);
        let surfaces = [SceneSurface {
            surface: Some(&surface),
            style: SurfaceStyle::Shaded,
            ..SceneSurface::default()
        }];
        let options = SceneOptions {
            vertical_exaggeration: 1.0e200,
            light_direction: Vec3::new(0.3, -0.4, 0.8),
            ..SceneOptions::default()
        };
        expect_same_terrain(kernel, &surfaces, &[], &options);

        let mut mesh = TriangleMesh::default();
        for k in 0..8 {
            mesh.vertices.push(Point3::new(
                if (k & 1) != 0 { 1.0e100 } else { 0.0 },
                if (k & 2) != 0 { 3.0e100 } else { 0.0 },
                if (k & 4) != 0 { 2.0e100 } else { 0.0 },
            ));
        }
        mesh.vertices.push(Point3::new(f64::NAN, 1.0, 2.0));
        mesh.faces = vec![
            [0, 2, 1],
            [1, 2, 3],
            [4, 5, 6],
            [5, 7, 6],
            [0, 1, 8],
            [0, 1, 4],
            [1, 5, 4],
            [2, 6, 3],
            [3, 6, 7],
        ];
        let items = [SceneMesh {
            mesh: Some(&mesh),
            ..SceneMesh::default()
        }];
        let plain = SceneOptions {
            light_direction: Vec3::new(0.3, -0.4, 0.8),
            ..SceneOptions::default()
        };
        expect_same_terrain(kernel, &[], &items, &plain);
    }

    #[test]
    fn edge_fades_equal_the_scalar_blend_for_every_channel_pair_and_every_eighth() {
        let kernel = require_kernels();
        // Every (base, ink) pair of channel values, each in all four channels
        // at once, with a count that ends mid-block. The kernel works in
        // integers; the scalar blend in doubles, so this is the proof they
        // agree.
        let mut layers = SceneLayers::default();
        for base in 0..256u32 {
            for ink in 0..256u32 {
                layers.edge_base.push(base * 0x0101_0101);
                layers.edge_ink.push(ink * 0x0101_0101);
            }
        }
        let count = layers.edge_base.len() - 3;
        layers.edge_base.truncate(count);
        layers.edge_ink.truncate(count);
        layers.edges.colors = layers.edge_base.clone();
        layers.edges.positions = vec![Vec3::new(0.0, 0.0, 0.0); count];
        let mut camera = Camera::default();
        camera.set_viewport_size(800, 600);
        assert!(camera.frame(AABB::new(
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(100.0, 100.0, 10.0)
        )));
        let pixel = camera.world_per_pixel_at(camera.distance());
        assert!(pixel > 0.0);
        // Typical edges of 4 to 12 px step through every eighth of the fade.
        let mut eighths_seen = 0;
        for tenths in (40..=120).step_by(2) {
            layers.edge_runs = vec![EdgeRun {
                first: 0,
                count: count as VertexIndex,
                typical_edge: pixel * f64::from(tenths) / 10.0,
                ..EdgeRun::default()
            }];
            let mut scalar = layers.clone();
            let mut kernels = layers.clone();
            {
                let _at = ScopedSimdLevel::new(SimdLevel::Scalar);
                SceneBuilder::fade_edges(&mut scalar, &camera);
            }
            {
                let _at = ScopedSimdLevel::new(kernel);
                SceneBuilder::fade_edges(&mut kernels, &camera);
            }
            assert_eq!(
                scalar.edge_runs[0].applied, kernels.edge_runs[0].applied,
                "{tenths}"
            );
            expect_same_bits(
                &scalar.edges.colors,
                &kernels.edges.colors,
                |&c| c,
                "faded colours",
            );
            eighths_seen |= 1 << (kernels.edge_runs[0].applied * 8.0) as i32;
        }
        assert_eq!(eighths_seen, 0x1FF, "every strength from 0 to 8 eighths");
    }

    #[test]
    fn the_bounds_are_the_box_of_every_list_even_where_an_extreme_is_a_signed_zero() {
        // The scene's bounds are its lists' DrawList::bounds(), which visits
        // each vertex once, not once per primitive (render's draw_list tests
        // hold it to the walk it replaced); the box must be theirs to the bit,
        // zeros' signs included.
        let surface = tin(
            vec![
                Point3::new(-0.0, 0.0, -0.0),
                Point3::new(10.0, -0.0, 0.0),
                Point3::new(10.0, 10.0, 5.0),
                Point3::new(0.0, 10.0, -0.0),
            ],
            vec![[0, 1, 2], [0, 2, 3]],
        );
        let surfaces = [SceneSurface {
            surface: Some(&surface),
            ..SceneSurface::default()
        }];
        let options = SceneOptions {
            exaggeration_datum: -0.0,
            ..SceneOptions::default()
        };
        let mut builder = SceneBuilder::new();
        let mut layers = SceneLayers::default();
        builder.build_terrain(&surfaces, &[], &options, &mut layers);
        let mut expected = layers.terrain.bounds();
        expected.expand(layers.edges.bounds());
        expect_same_box(&expected, &layers.bounds, "terrain bounds");
        assert_eq!(layers.bounds.max.x, 10.0);
        assert_eq!(layers.bounds.max.z, 5.0);
    }
}
