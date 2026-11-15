// The 3D scene colouring a surface by its display style (scene.rs's
// SceneSurface::display, docs/surface_display.md "In 3D";
// tests/cad/terrain/test_scene_surface_style.cpp): the colours expected are
// the ramp table's stops and hand-worked band cuts, drawn with the light off
// so a colour is the style's own.

use crate::tin;
use katana_cad::scene::{
    SceneBuilder, SceneLayers, SceneOptions, SceneSurface, SurfaceColoring, apply_surface_display,
};
use katana_cad::terrain::surface_colouring::aspect_sector_colour;
use katana_cad::terrain::surface_style::{
    ColourStop, SurfaceDisplayStyle, SurfaceFaces, SurfaceStyle,
};
use katana_entity::Color;
use katana_geometry::Point3;
use katana_math::Vec3;
use katana_render::{DrawList, Rgba, rgba, rgba_with_alpha};
use katana_terrain::TinSurface;

/// The square 10 x 10 from (0, 0), two triangles, z from `height`.
fn square(height: impl Fn(f64, f64) -> f64) -> TinSurface {
    let vertices = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]
        .into_iter()
        .map(|(x, y)| Point3::new(x, y, height(x, y)))
        .collect();
    tin(vertices, vec![[0, 1, 2], [0, 2, 3]])
}

fn rolling(cells: i32) -> TinSurface {
    let mut vertices = Vec::new();
    for j in 0..=cells {
        for i in 0..=cells {
            vertices.push(Point3::new(
                4.0 * f64::from(i),
                4.0 * f64::from(j),
                20.0 + 3.0 * (f64::from(i) * 0.7).sin() + 2.0 * (f64::from(j) * 0.4).cos(),
            ));
        }
    }
    let at = |i: i32, j: i32| (j * (cells + 1) + i) as u32;
    let mut triangles = Vec::new();
    for j in 0..cells {
        for i in 0..cells {
            triangles.push([at(i, j), at(i + 1, j), at(i + 1, j + 1)]);
            triangles.push([at(i, j), at(i + 1, j + 1), at(i, j + 1)]);
        }
    }
    tin(vertices, triangles)
}

fn unlit() -> SceneOptions<'static> {
    SceneOptions {
        draw_grid: false,
        // The style's colours as they are.
        light_direction: Vec3::new(0.0, 0.0, 0.0),
        ..SceneOptions::default()
    }
}

fn styled(faces: SurfaceFaces) -> SurfaceDisplayStyle {
    SurfaceDisplayStyle {
        name: String::from("test"),
        shading: SurfaceStyle::Shaded,
        faces3d: faces,
        ..SurfaceDisplayStyle::default()
    }
}

fn item_of<'a>(surface: &'a TinSurface, style: &SurfaceDisplayStyle) -> SceneSurface<'a> {
    let mut item = SceneSurface {
        name: String::from("ground"),
        surface: Some(surface),
        ..SceneSurface::default()
    };
    apply_surface_display(&mut item, style);
    item
}

/// Each triangle of a list: its three corners' colour (asserted equal) and
/// its corners' least and greatest x.
struct Face {
    colour: Rgba,
    low_x: f64,
    high_x: f64,
}

fn faces_of(list: &DrawList) -> Vec<Face> {
    let mut faces = Vec::new();
    for triangle in &list.triangles {
        let corners = [triangle.a, triangle.b, triangle.c];
        let mut face = Face {
            colour: list.colors[corners[0] as usize],
            low_x: 1e300,
            high_x: -1e300,
        };
        for corner in corners {
            assert_eq!(list.colors[corner as usize], face.colour);
            face.low_x = katana_math::cpp::min(face.low_x, list.positions[corner as usize].x);
            face.high_x = katana_math::cpp::max(face.high_x, list.positions[corner as usize].x);
        }
        faces.push(face);
    }
    faces
}

mod scene_surface_style {
    use super::*;

    #[test]
    fn a_style_on_the_terrain_ramp_colours_a_surface_as_the_scene_always_did() {
        let surface = rolling(12);
        for shading in [SurfaceStyle::Shaded, SurfaceStyle::ShadedWithEdges] {
            let legacy = SceneSurface {
                name: String::from("ground"),
                surface: Some(&surface),
                style: shading,
                coloring: SurfaceColoring::Elevation,
                ..SceneSurface::default()
            };
            let mut style = styled(SurfaceFaces::Elevation);
            style.shading = shading;
            style.ramp = String::from("terrain");
            let item = item_of(&surface, &style);
            let mut builder = SceneBuilder::new();
            let mut before = SceneLayers::default();
            let mut after = SceneLayers::default();
            builder.build_terrain(&[legacy], &[], &SceneOptions::default(), &mut before);
            builder.build_terrain(&[item], &[], &SceneOptions::default(), &mut after);
            assert_eq!(
                before.terrain.positions.len(),
                after.terrain.positions.len()
            );
            assert_eq!(before.terrain.colors, after.terrain.colors);
            assert_eq!(
                before.terrain.triangles.len(),
                after.terrain.triangles.len()
            );
            assert_eq!(before.edges.colors, after.edges.colors);
            assert_eq!(before.ramp_low, after.ramp_low);
            assert_eq!(before.ramp_high, after.ramp_high);
        }
    }

    #[test]
    fn a_banded_style_cuts_each_triangle_at_its_band_edge_and_colours_each_part_its_band() {
        // z = x from 0 to 10 in two bands, cut at 5. Triangle (0,0) (10,0)
        // (10,10) gives a triangle below 5 and a quadrilateral above (three
        // triangles drawn); (0,0) (10,10) (0,10) a quadrilateral below and a
        // triangle above (three more): six.
        let surface = square(|x, _| x);
        let mut style = styled(SurfaceFaces::Elevation);
        style.banded = true;
        style.bands = 2;
        let mut builder = SceneBuilder::new();
        let mut list = DrawList::default();
        builder.append_surfaces(&[item_of(&surface, &style)], &unlit(), &mut list);
        let faces = faces_of(&list);
        assert_eq!(faces.len(), 6);
        // Band 0 is the terrain ramp's start, band 1 its end.
        for face in &faces {
            if face.high_x <= 5.0 + 1e-12 {
                assert_eq!(face.colour, rgba(40, 70, 140));
            } else {
                assert!(face.low_x >= 5.0 - 1e-12);
                assert_eq!(face.colour, rgba(245, 245, 245));
            }
        }
    }

    #[test]
    fn slope_faces_take_the_styles_range_and_ramp() {
        // z = x / 2: every face 50 %, the middle of 0 to 100, which on the
        // grey ramp is its middle stop (128, 128, 128).
        let surface = square(|x, _| x / 2.0);
        let mut style = styled(SurfaceFaces::Slope);
        style.ramp = String::from("grey");
        style.range_min = Some(0.0);
        style.range_max = Some(100.0);
        let mut builder = SceneBuilder::new();
        let mut list = DrawList::default();
        builder.append_surfaces(&[item_of(&surface, &style)], &unlit(), &mut list);
        let faces = faces_of(&list);
        assert_eq!(faces.len(), 2);
        for face in &faces {
            assert_eq!(face.colour, rgba(128, 128, 128));
        }
    }

    #[test]
    fn aspect_faces_are_the_colour_of_the_sector_they_look_to() {
        // z = 10 - y falls towards the north: aspect 0, sector N.
        let surface = square(|_, y| 10.0 - y);
        let mut builder = SceneBuilder::new();
        let mut list = DrawList::default();
        builder.append_surfaces(
            &[item_of(&surface, &styled(SurfaceFaces::Aspect))],
            &unlit(),
            &mut list,
        );
        let north = aspect_sector_colour(0);
        for face in faces_of(&list) {
            assert_eq!(
                face.colour,
                rgba_with_alpha(north.r, north.g, north.b, north.a)
            );
        }
    }

    #[test]
    fn smooth_stops_colour_each_vertex_by_its_height() {
        let surface = square(|x, _| x);
        let mut style = styled(SurfaceFaces::Elevation);
        style.stops = vec![
            ColourStop {
                value: 0.0,
                colour: Color::new(0, 0, 0, 255),
            },
            ColourStop {
                value: 10.0,
                colour: Color::new(200, 100, 50, 255),
            },
        ];
        let mut builder = SceneBuilder::new();
        let mut list = DrawList::default();
        builder.append_surfaces(&[item_of(&surface, &style)], &unlit(), &mut list);
        assert_eq!(list.positions.len(), 4);
        for (&colour, position) in list.colors.iter().zip(&list.positions) {
            assert_eq!(
                colour,
                if position.x == 0.0 {
                    rgba(0, 0, 0)
                } else {
                    rgba(200, 100, 50)
                }
            );
        }
    }

    #[test]
    fn a_surface_with_a_range_of_its_own_does_not_stretch_the_one_ramp() {
        let low = square(|x, _| x); // 0 to 10
        let high = square(|x, _| 100.0 + 10.0 * x); // 100 to 200
        let plain = SceneSurface {
            name: String::from("low"),
            surface: Some(&low),
            style: SurfaceStyle::Shaded,
            ..SceneSurface::default()
        };
        let mut own = styled(SurfaceFaces::Elevation);
        own.range_min = Some(100.0);
        own.range_max = Some(200.0);
        let mut builder = SceneBuilder::new();
        let mut layers = SceneLayers::default();
        builder.build_terrain(&[plain, item_of(&high, &own)], &[], &unlit(), &mut layers);
        assert_eq!(layers.ramp_low, 0.0);
        assert_eq!(layers.ramp_high, 10.0);
    }

    #[test]
    fn a_surface_in_a_style_has_a_legend_of_its_colours_and_one_with_no_style_has_the_ramps_bar() {
        let surface = square(|x, _| x);
        let mut style = styled(SurfaceFaces::Elevation);
        style.name = String::from("Bands");
        style.banded = true;
        style.bands = 4;
        let mut builder = SceneBuilder::new();
        let mut layers = SceneLayers::default();
        builder.build_terrain(&[item_of(&surface, &style)], &[], &unlit(), &mut layers);
        assert_eq!(layers.legends.len(), 1);
        assert_eq!(layers.legends[0].legend.title, "Elevation");
        assert_eq!(layers.legends[0].legend.entries.len(), 4);
        assert_eq!(layers.legends[0].surface, "ground");
        assert_eq!(layers.legends[0].style, "Bands");
        // A surface with no style has the one ramp's legend, not this one.
        let plain = SceneSurface {
            surface: Some(&surface),
            ..SceneSurface::default()
        };
        builder.build_terrain(&[plain], &[], &unlit(), &mut layers);
        assert!(layers.legends.is_empty());
        assert!(layers.has_ramp());
    }

    #[test]
    fn every_visible_surface_in_a_colouring_style_has_a_legend_in_drawing_order_and_no_other_has_one()
     {
        // Five surfaces, in the order they are drawn:
        //   "first"   elevation in 4 bands          - a legend, titled Elevation, 4 entries;
        //   "hidden"  the same, but not visible     - not drawn, no legend;
        //   "plain"   no display style              - the one ramp's bar, no legend;
        //   "mesh"    a wireframe (its mesh is one grey) - no legend;
        //   "third"   slope in 5 bands              - a legend, titled Slope %, 5 entries.
        let surface = square(|x, _| x);
        let mut bands = styled(SurfaceFaces::Elevation);
        bands.name = String::from("Bands");
        bands.banded = true;
        bands.bands = 4;
        let mut slope = styled(SurfaceFaces::Slope);
        slope.name = String::from("Steep");
        slope.banded = true;
        slope.bands = 5;
        let mut outline = styled(SurfaceFaces::Elevation);
        outline.name = String::from("Outline");
        outline.shading = SurfaceStyle::Wireframe;

        let mut first = item_of(&surface, &bands);
        first.name = String::from("first");
        let mut hidden = item_of(&surface, &bands);
        hidden.name = String::from("hidden");
        hidden.visible = false;
        let plain = SceneSurface {
            name: String::from("plain"),
            surface: Some(&surface),
            ..SceneSurface::default()
        };
        let mut mesh = item_of(&surface, &outline);
        mesh.name = String::from("mesh");
        let mut third = item_of(&surface, &slope);
        third.name = String::from("third");

        let mut builder = SceneBuilder::new();
        let mut layers = SceneLayers::default();
        builder.build_terrain(
            &[first, hidden, plain.clone(), mesh, third],
            &[],
            &unlit(),
            &mut layers,
        );
        assert_eq!(layers.legends.len(), 2);
        assert_eq!(layers.legends[0].surface, "first");
        assert_eq!(layers.legends[0].style, "Bands");
        assert_eq!(layers.legends[0].legend.title, "Elevation");
        assert_eq!(layers.legends[0].legend.entries.len(), 4);
        assert_eq!(layers.legends[1].surface, "third");
        assert_eq!(layers.legends[1].style, "Steep");
        assert_eq!(layers.legends[1].legend.title, "Slope %");
        assert_eq!(layers.legends[1].legend.entries.len(), 5);

        // A rebuild starts from nothing: the surfaces taken away, no legend
        // stays.
        builder.build_terrain(&[plain], &[], &unlit(), &mut layers);
        assert!(layers.legends.is_empty());
        builder.build_terrain(&[], &[], &unlit(), &mut layers);
        assert!(layers.legends.is_empty());
    }

    #[test]
    fn applying_a_style_sets_the_shading_and_keeps_a_copy_of_the_style() {
        let surface = square(|x, _| x);
        let mut style = styled(SurfaceFaces::Slope);
        style.shading = SurfaceStyle::Wireframe;
        style.flat_colour = Color::new(1, 2, 3, 255);
        let item = item_of(&surface, &style);
        assert_eq!(item.style, SurfaceStyle::Wireframe);
        assert_eq!(item.display.as_ref(), Some(&style));
        assert_eq!(item.coloring, SurfaceColoring::Slope);
        assert_eq!(item.flat_color, rgba(1, 2, 3));
        // A copy: changing the style afterwards changes nothing drawn.
        style.ramp = String::from("grey");
        assert_eq!(
            item.display.as_ref().map(|display| display.ramp.as_str()),
            Some("terrain")
        );
    }

    #[test]
    fn a_wireframe_colours_nothing_so_its_heights_stretch_no_ramp_and_alone_it_brings_no_bar() {
        // ground 0 to 10 shaded on the one ramp; design 100 to 200 drawn as a
        // grey mesh (the built-in Outline style). The mesh is one grey
        // whatever its heights, so they are no part of the range the shaded
        // surface is coloured over: ground's top, at 10, is the ramp's end
        // (245, 245, 245). Counted in, the range was 0 to 200 and ground's top
        // a twentieth of the way along it, nearly the ramp's start; the bar
        // read 200.
        let ground = square(|x, _| x);
        let design = square(|x, _| 100.0 + 10.0 * x);
        let mut outline = styled(SurfaceFaces::Elevation);
        outline.shading = SurfaceStyle::Wireframe;
        let mut builder = SceneBuilder::new();
        let mut layers = SceneLayers::default();
        builder.build_terrain(
            &[
                item_of(&ground, &styled(SurfaceFaces::Elevation)),
                item_of(&design, &outline),
            ],
            &[],
            &unlit(),
            &mut layers,
        );
        assert_eq!(layers.ramp_low, 0.0);
        assert_eq!(layers.ramp_high, 10.0);
        assert!(layers.terrain.colors.contains(&rgba(245, 245, 245)));
        assert!(layers.terrain.colors.contains(&rgba(40, 70, 140)));

        // Alone, the mesh brings up no elevation bar: nothing in view is
        // coloured by elevation. (It was the old bar, 200 / 100, over a grey
        // mesh.)
        builder.build_terrain(&[item_of(&design, &outline)], &[], &unlit(), &mut layers);
        assert!(!layers.has_ramp());
        assert!(layers.legends.is_empty());

        // And a surface with no style, wireframe, in the scene's own rule from
        // before styles.
        let plain = SceneSurface {
            name: String::from("design"),
            surface: Some(&design),
            style: SurfaceStyle::Wireframe,
            coloring: SurfaceColoring::Elevation,
            ..SceneSurface::default()
        };
        builder.build_terrain(&[plain], &[], &unlit(), &mut layers);
        assert!(!layers.has_ramp());
    }

    #[test]
    fn a_range_above_the_whole_surface_colours_its_vertices_the_ramps_start_not_its_middle() {
        // z = x is 0 to 10 and min=40 is above all of it: the range collapses
        // to the point 40 with every height below it, so every vertex is the
        // ramp's start (40, 70, 140). The kernels' ramp has only a middle for
        // a range that narrow, (235, 220, 130), and drew it for all of them.
        let surface = square(|x, _| x);
        let mut style = styled(SurfaceFaces::Elevation);
        style.range_min = Some(40.0);
        let mut builder = SceneBuilder::new();
        let mut list = DrawList::default();
        builder.append_surfaces(&[item_of(&surface, &style)], &unlit(), &mut list);
        assert_eq!(list.colors.len(), 4);
        for &colour in &list.colors {
            assert_eq!(colour, rgba(40, 70, 140));
        }
    }

    #[test]
    fn a_level_face_coloured_by_slope_is_the_slope_ramps_start_as_it_always_was() {
        // The old 3D slope line gave 0 % the ramp's green start, (0, 255, 38);
        // the style's range is 0 to the steepest face, which on a level pad is
        // 0, and the middle (255, 255, 38) was drawn for it.
        let surface = square(|_, _| 5.0);
        let mut style = styled(SurfaceFaces::Slope);
        style.ramp = String::from("slope");
        let mut builder = SceneBuilder::new();
        let mut list = DrawList::default();
        builder.append_surfaces(&[item_of(&surface, &style)], &unlit(), &mut list);
        let faces = faces_of(&list);
        assert_eq!(faces.len(), 2);
        for face in &faces {
            assert_eq!(face.colour, rgba(0, 255, 38));
        }
    }
}
