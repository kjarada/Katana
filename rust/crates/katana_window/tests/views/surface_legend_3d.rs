//! The twins of tests/qt_widgets/terrain/test_surface_legend_3d.cpp (wave 4's stragglers): the 3D
//! view's legend of a surface drawn in a display style (render_view_widget's draw_legend,
//! docs/surface_display.md "In 3D"): the style's own colours, drawn with the plan view's legend
//! panel, where a surface with no style has the one elevation ramp's bar.
//!
//! The C++ opens its 3D view through the workspace and hands it the surfaces (setSurfaces); here
//! the workspace opens it and lends it the window's surfaces at each call (Scenery), as the window
//! does. The C++'s widget resize and paint are the view's own `resize` and `paint`, and its grab is
//! the picture that paint drew.

use katana_cad::Document;
use katana_cad::scene::{SceneSurface, apply_surface_display};
use katana_cad::terrain::surface_style::{SurfaceDisplayStyle, SurfaceFaces};
use katana_cad::viewport_layout::ViewKind;
use katana_geometry::Point3;
use katana_terrain::{TinBuildOptions, TinInput, TinSurface, build_tin};
use katana_window::canvas::{Color, Image};
use katana_window::terrain::surface_plan_painter::DrawnLegend;
use katana_window::view_workspace::{Scenery, ViewWorkspace};

use crate::support::double_eq;

/// Whether `image` has a pixel of exactly `colour` in its top-right quarter, where the legend is
/// drawn. A legend swatch is the band's colour unlit; the surface's own faces are lit (a flat face
/// takes 0.865 of its colour under the default sun), so an exact match there is the legend's.
fn corner_has(image: &Image, colour: Color) -> bool {
    for y in 0..image.height() / 2 {
        for x in image.width() / 2..image.width() {
            let pixel = image.pixel(x, y);
            if (pixel.r, pixel.g, pixel.b) == (colour.r, colour.g, colour.b) {
                return true;
            }
        }
    }
    false
}

/// A TIN over a 100 x 100 square with a hump, whose relief gives a style something to band.
fn hill() -> TinSurface {
    let mut input = TinInput::default();
    for (x, y, z) in [
        (0.0, 0.0, 10.0),
        (100.0, 0.0, 12.0),
        (100.0, 100.0, 14.0),
        (0.0, 100.0, 11.0),
        (50.0, 50.0, 13.0),
    ] {
        input.points.push(Point3::new(x, y, z));
    }
    build_tin(&input, &TinBuildOptions::default())
        .unwrap_or_else(|error| panic!("{}", error.describe()))
        .surface
}

fn bands_style(name: &str, faces: SurfaceFaces, bands: i32, ramp: &str) -> SurfaceDisplayStyle {
    SurfaceDisplayStyle {
        name: String::from(name),
        ramp: String::from(ramp),
        faces3d: faces,
        banded: true,
        bands,
        ..SurfaceDisplayStyle::default()
    }
}

/// What one 3D view over `surfaces`, painted at 600 x `height_pixels`, drew.
struct Painted {
    picture: Image,
    drawn: Vec<DrawnLegend>,
    scene_legends: usize,
    legend_entries: Vec<usize>,
    legend_styles: Vec<String>,
    has_ramp: bool,
    width: f64,
    height: f64,
}

/// One 3D view in a 900 x 700 window, opened by the workspace, lent `surfaces`, resized to
/// 600 x `height_pixels` and painted (the C++'s LegendView).
fn legend_view(surfaces: &[SceneSurface<'_>], height_pixels: i32) -> Painted {
    let mut document = Document::new(None);
    let views = ViewWorkspace::new(&document);
    views.set_area(egui::Rect::from_min_size(
        egui::Pos2::ZERO,
        egui::vec2(900.0, 700.0),
    ));
    // setSurfaces: the views are told the window's surfaces changed.
    views.scenery_changed();
    let model = views.open_view(&mut document, ViewKind::Model3D, true);
    let scenery = Scenery {
        surfaces,
        meshes: &[],
    };
    views
        .with_render_view(model, &document, scenery, |view, scene, state| {
            view.resize(scene, state, 600, height_pixels);
            let picture = view.paint(scene, state).clone();
            let layers = view.scene_layers();
            let (width, height) = view.size();
            Painted {
                picture,
                drawn: view.drawn_legends().to_vec(),
                scene_legends: layers.legends.len(),
                legend_entries: layers
                    .legends
                    .iter()
                    .map(|card| card.legend.entries.len())
                    .collect(),
                legend_styles: layers.legends.iter().map(|card| card.style.clone()).collect(),
                has_ramp: layers.has_ramp(),
                width: f64::from(width),
                height: f64::from(height),
            }
        })
        .expect("a 3D view")
}

/// The terrain ramp's start, band 0 of elevation bands.
const TERRAIN_START: Color = Color::rgb(40, 70, 140);
/// The slope ramp's start, band 0 of slope bands.
const SLOPE_START: Color = Color::rgb(0, 255, 38);

mod surface_legend3_d {
    use super::*;

    #[test]
    fn a_surface_in_a_banded_style_shows_its_bands_in_the_three_d_views_legend() {
        let built = hill();
        let mut surface = SceneSurface {
            name: String::from("Ground"),
            surface: Some(&built),
            ..SceneSurface::default()
        };
        // The C++'s style here leaves the ramp as a new style has it.
        let style = SurfaceDisplayStyle {
            name: String::from("Bands"),
            faces3d: SurfaceFaces::Elevation,
            banded: true,
            bands: 4,
            ..SurfaceDisplayStyle::default()
        };
        apply_surface_display(&mut surface, &style);
        let surfaces = [surface];

        let view = legend_view(&surfaces, 450);
        assert_eq!(view.scene_legends, 1);
        assert_eq!(view.legend_entries[0], 4);
        assert_eq!(view.legend_styles[0], "Bands");
        assert_eq!(view.drawn.len(), 1);
        assert_eq!(view.drawn[0].caption, "Ground - Bands");
        // Band 0 is the terrain ramp's start, (40, 70, 140), its swatch in the legend panel at
        // the view's top-right.
        assert!(corner_has(&view.picture, TERRAIN_START));
    }

    #[test]
    fn each_visible_styled_surface_has_its_legend_stacked_in_drawing_order_and_no_other_has_one()
    {
        let built = hill();
        // In drawing order: "Ground" in elevation bands, "Hidden" in the same but not visible,
        // "Plain" with no display style (the one ramp's bar), and "Design" in slope bands. Two
        // legends, Ground's above Design's.
        let named = |name: &str| SceneSurface {
            name: String::from(name),
            surface: Some(&built),
            ..SceneSurface::default()
        };
        let mut ground = named("Ground");
        apply_surface_display(
            &mut ground,
            &bands_style("Bands", SurfaceFaces::Elevation, 4, "terrain"),
        );
        let mut hidden = named("Hidden");
        apply_surface_display(
            &mut hidden,
            &bands_style("Bands", SurfaceFaces::Elevation, 4, "terrain"),
        );
        hidden.visible = false;
        let plain = named("Plain");
        let mut design = named("Design");
        apply_surface_display(
            &mut design,
            &bands_style("Steep", SurfaceFaces::Slope, 3, "slope"),
        );
        let surfaces = [ground, hidden, plain, design];

        let view = legend_view(&surfaces, 450);
        let drawn = &view.drawn;
        assert_eq!(drawn.len(), 2);
        assert_eq!(drawn[0].caption, "Ground - Bands");
        assert_eq!(drawn[1].caption, "Design - Steep");
        // Down the top-right corner, 16 px in: one panel above the other with a gap between,
        // neither over the view's edge, their right edges together.
        assert!(double_eq(drawn[0].panel.top(), 16.0), "{}", drawn[0].panel.top());
        assert!(
            double_eq(drawn[0].panel.right(), view.width - 16.0),
            "{}",
            drawn[0].panel.right()
        );
        assert!(
            double_eq(drawn[1].panel.right(), view.width - 16.0),
            "{}",
            drawn[1].panel.right()
        );
        assert!(drawn[0].panel.bottom() < drawn[1].panel.top());
        assert!(drawn[1].panel.bottom() <= view.height - 16.0);
        // Each panel is its own style's colours, unlit, in the corner: band 0 of the elevation
        // bands is the terrain ramp's start, (40, 70, 140), and of the slope bands the slope
        // ramp's, (0, 255, 38).
        assert!(corner_has(&view.picture, TERRAIN_START));
        assert!(corner_has(&view.picture, SLOPE_START));
        assert_eq!(view.scene_legends, 2);
    }

    #[test]
    fn with_no_surface_in_a_style_there_is_no_legend_and_the_one_ramp_keeps_its_bar() {
        let built = hill();
        let surfaces = [SceneSurface {
            name: String::from("Ground"),
            surface: Some(&built),
            ..SceneSurface::default()
        }];
        let view = legend_view(&surfaces, 450);
        assert!(view.drawn.is_empty());
        assert_eq!(view.scene_legends, 0);
        assert!(view.has_ramp);
    }

    #[test]
    fn more_legends_than_the_view_holds_are_left_out_whole_not_cut_off_or_overlapped() {
        // Eight surfaces in banded styles, each legend a caption, a title and four rows - well
        // over 80 px - in a view 300 px high with 16 px margins: room for the first few, so the
        // later ones are left out, whole.
        let built = hill();
        let surfaces: Vec<SceneSurface<'_>> = (1..=8)
            .map(|i| {
                let mut surface = SceneSurface {
                    name: format!("S{i}"),
                    surface: Some(&built),
                    ..SceneSurface::default()
                };
                apply_surface_display(
                    &mut surface,
                    &bands_style("Bands", SurfaceFaces::Elevation, 4, "terrain"),
                );
                surface
            })
            .collect();
        let view = legend_view(&surfaces, 300);
        let drawn = &view.drawn;
        assert!(!drawn.is_empty());
        assert!(drawn.len() < surfaces.len());
        for (i, legend) in drawn.iter().enumerate() {
            assert_eq!(
                legend.caption,
                format!("S{} - Bands", i + 1),
                "in drawing order"
            );
            if i > 0 {
                assert!(drawn[i - 1].panel.bottom() < legend.panel.top());
                assert!(
                    legend.panel.bottom() <= view.height - 16.0,
                    "cut by the edge"
                );
            }
        }
        // The scene still holds all eight: it is the view that has no more room.
        assert_eq!(view.scene_legends, surfaces.len());
    }
}
