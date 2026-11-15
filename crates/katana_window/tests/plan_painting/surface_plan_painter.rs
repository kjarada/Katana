//! The twin of tests/qt_widgets/terrain/test_surface_plan_painter.cpp: the drawing's surfaces in
//! the plan painter (terrain::surface_plan_painter, plan_painter; docs/surface_display.md "In
//! plan") - a surface drawn where nothing was, in its style, beneath the entities, through the one
//! painter the plan view, plots and sheets share. Ink is counted against the same paint without
//! the surface; where a count is exact it is the square's own pixels, worked out from the frame,
//! never a previous run's.

use std::collections::BTreeSet;
use std::sync::Arc;

use katana_cad::document::Document;
use katana_cad::layer_overrides::LayerOverrides;
use katana_cad::plot::PlotSettings;
use katana_cad::terrain::surface_colouring::{surface_colouring, surface_legend};
use katana_cad::terrain::surface_style::{
    SurfaceDisplayStyle, SurfaceFaces, SurfaceStyleSet, built_in_surface_styles,
};
use katana_entity::Model;
use katana_geometry::{Point2, Point3};
use katana_terrain::surface_store::{NamedSurface, SurfaceStore};
use katana_terrain::tin_surface::TinSurface;
use katana_window::canvas::{Color, Font, FontSize, Image, PointF, RectF};
use katana_window::plan_painter::{
    PlanFrame, PlanMedium, PlanPaintCache, PlanPaintOptions, PlanPaintStats, PlanSource,
    paint_plan, plan_drawn_bounds, plan_source_of,
};
use katana_window::terrain::surface_plan_painter::{measure_surface_legend, paint_surface_legend};

use crate::raster::{metrics, paint_on};

const BLACK: Color = Color::BLACK;

/// The 10 x 10 square z = x, two triangles.
fn square() -> Arc<TinSurface> {
    let tin = TinSurface::create(
        vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(10.0, 0.0, 10.0),
            Point3::new(10.0, 10.0, 10.0),
            Point3::new(0.0, 10.0, 0.0),
        ],
        vec![[0, 1, 2], [0, 2, 3]],
        Vec::new(),
    )
    .expect("the square is a surface");
    Arc::new(tin)
}

fn bare(name: &str) -> SurfaceDisplayStyle {
    let mut style = SurfaceDisplayStyle {
        name: String::from(name),
        show_boundary: false,
        plan_faces: SurfaceFaces::None,
        ..SurfaceDisplayStyle::default()
    };
    style.contours.show = false;
    style
}

/// "Bands": opaque elevation faces in five bands of 2; "Outline": the rim alone; "Contours":
/// contours every 1, no faces; "Triangles": the edges.
fn styles() -> SurfaceStyleSet {
    let bands = SurfaceDisplayStyle {
        plan_faces: SurfaceFaces::Elevation,
        banded: true,
        bands: 5,
        plan_opacity: 1.0,
        ..bare("Bands")
    };
    let outline = SurfaceDisplayStyle {
        show_boundary: true,
        ..bare("Outline")
    };
    let mut contours = bare("Contours");
    contours.contours.show = true;
    contours.contours.interval = 1.0;
    let triangles = SurfaceDisplayStyle {
        show_triangles: true,
        ..bare("Triangles")
    };
    SurfaceStyleSet {
        styles: vec![bands, outline, contours, triangles],
        default_style: String::from("Bands"),
    }
}

/// An eighth of a unit a pixel, exact in binary: the square's corner (0, 10) lands on pixel
/// (8, 8) of a 96 x 96 frame about (5, 5), and the square covers pixels 8 to 87 each way - 80 x 80.
fn frame() -> PlanFrame<'static> {
    let mut frame = PlanFrame::default();
    frame.transform.resize(96.0, 96.0);
    frame.transform.scale = 8.0;
    frame.transform.center = Point2::new(5.0, 5.0);
    frame
}

fn paint_with(
    surfaces: Option<&SurfaceStore>,
    set: &SurfaceStyleSet,
    options: &PlanPaintOptions,
    cache: &mut PlanPaintCache,
) -> (Image, PlanPaintStats) {
    let empty = Model::new();
    let source = PlanSource {
        model: Some(&empty),
        surfaces,
        surface_styles: Some(set),
        ..PlanSource::default()
    };
    let mut image = Image::new(96, 96);
    image.fill(BLACK);
    let mut stats = PlanPaintStats::default();
    let frame = frame();
    paint_on(&mut image, |canvas| {
        stats = paint_plan(canvas, &source, &frame, options, cache);
    });
    (image, stats)
}

fn paint(
    surfaces: Option<&SurfaceStore>,
    set: &SurfaceStyleSet,
    options: &PlanPaintOptions,
) -> (Image, PlanPaintStats) {
    paint_with(surfaces, set, options, &mut PlanPaintCache::default())
}

fn store_in(style: &str, visible: bool) -> SurfaceStore {
    let mut store = SurfaceStore::new();
    store
        .add(NamedSurface {
            name: String::from("ground"),
            surface: Some(square()),
            source: String::from("test"),
            style: String::from(style),
            visible,
        })
        .expect("the surface is added");
    store
}

fn differing(a: &Image, b: &Image) -> usize {
    let mut count = 0;
    for y in 0..a.height() {
        for x in 0..a.width() {
            count += usize::from(a.pixel(x, y) != b.pixel(x, y));
        }
    }
    count
}

/// The part of a pixel's colour that is not alpha (QColor of a QRgb ignores the alpha).
fn ink_at(image: &Image, x: u32, y: u32) -> (u8, u8, u8) {
    let c = image.pixel(x, y);
    (c.r, c.g, c.b)
}

fn channel_gap(a: (u8, u8, u8), b: (u8, u8, u8)) -> i32 {
    let gap = |p: u8, q: u8| (i32::from(p) - i32::from(q)).abs();
    gap(a.0, b.0).max(gap(a.1, b.1)).max(gap(a.2, b.2))
}

fn no_legends() -> PlanPaintOptions<'static> {
    // Screen furniture, tested on its own below.
    PlanPaintOptions {
        surface_legends: false,
        ..PlanPaintOptions::default()
    }
}

mod surface_plan_painter {
    use super::*;

    #[test]
    fn a_surface_in_its_style_inks_exactly_its_own_pixels_where_nothing_was_before() {
        let options = no_legends();
        let set = styles();
        let store = store_in("Bands", true);
        let (without, _) = paint(None, &set, &options);
        let (with, stats) = paint(Some(&store), &set, &options);
        // Opaque faces over the square's 80 x 80 pixels, each a device pixel of the image, laid on
        // whole pixels: every one changed, and nothing else.
        assert_eq!(stats.surfaces_drawn, 1);
        assert_eq!(stats.surface_triangles_in_view, 2);
        assert_eq!(stats.surface_pixels_covered, 6400);
        let mut inside = 0;
        for y in 8..88 {
            for x in 8..88 {
                inside += usize::from(with.pixel(x, y) != without.pixel(x, y));
            }
        }
        assert_eq!(inside, 6400);
        assert_eq!(differing(&with, &without), 6400);
    }

    #[test]
    fn a_face_in_a_known_band_is_drawn_in_that_bands_colour() {
        let set = styles();
        let store = store_in("Bands", true);
        let (with, _) = paint(Some(&store), &set, &no_legends());
        // Pixel column 8 + 4 is at x = 4.5 / 8 = 0.5625: band 0, the terrain ramp's start
        // (40, 70, 140). Column 8 + 28, x = 3.5625: band 1, the ramp at a quarter, (60, 160, 90).
        // Column 8 + 76: band 4, (245, 245, 245).
        assert_eq!(ink_at(&with, 12, 48), (40, 70, 140));
        assert_eq!(ink_at(&with, 36, 48), (60, 160, 90));
        assert_eq!(ink_at(&with, 84, 20), (245, 245, 245));
    }

    #[test]
    fn each_style_inks_its_own_way() {
        let options = no_legends();
        let set = styles();
        let (baseline, _) = paint(None, &set, &options);
        let names = ["Bands", "Outline", "Contours", "Triangles"];
        let mut ink = [0usize; 4];
        for (i, name) in names.iter().enumerate() {
            let store = store_in(name, true);
            ink[i] = differing(&paint(Some(&store), &set, &options).0, &baseline);
            assert!(ink[i] > 0, "{name}");
        }
        // Faces cover the square; lines only cross it.
        assert!(ink[0] > ink[1]);
        assert!(ink[0] > ink[2]);
        assert!(ink[0] > ink[3]);
        // Nine minor contours across the square ink more than its rim.
        assert!(ink[2] > ink[1]);
    }

    #[test]
    fn a_hidden_surface_draws_nothing_and_is_not_in_the_extents() {
        let set = styles();
        let hidden = store_in("Bands", false);
        let shown = store_in("Bands", true);
        let options = PlanPaintOptions::default();
        let (baseline, _) = paint(None, &set, &options);
        let (with, stats) = paint(Some(&hidden), &set, &options);
        assert_eq!(differing(&with, &baseline), 0);
        assert_eq!(stats.surfaces_drawn, 0);

        let empty = Model::new();
        let mut source = PlanSource {
            model: Some(&empty),
            surface_styles: Some(&set),
            surfaces: Some(&hidden),
            ..PlanSource::default()
        };
        assert!(plan_drawn_bounds(&source, &LayerOverrides::default(), &BTreeSet::new()).empty());
        source.surfaces = Some(&shown);
        let bounds = plan_drawn_bounds(&source, &LayerOverrides::default(), &BTreeSet::new());
        assert_eq!(bounds.min, Point2::new(0.0, 0.0));
        assert_eq!(bounds.max, Point2::new(10.0, 10.0));
    }

    #[test]
    fn the_legend_is_screen_furniture_and_never_plotted() {
        let set = styles();
        let store = store_in("Bands", true);
        assert_eq!(
            paint(Some(&store), &set, &PlanPaintOptions::default())
                .1
                .surface_legends,
            1
        );
        let plot = PlotSettings::default();
        let paper = PlanPaintOptions {
            medium: PlanMedium::Paper,
            pixels_per_millimetre: 4.0,
            plot: Some(&plot),
            ..PlanPaintOptions::default()
        };
        let (_, plotted) = paint(Some(&store), &set, &paper);
        assert_eq!(plotted.surfaces_drawn, 1);
        assert_eq!(plotted.surface_legends, 0);
        // A style without faces has no legend on screen either.
        let outline = store_in("Outline", true);
        assert_eq!(
            paint(Some(&outline), &set, &PlanPaintOptions::default())
                .1
                .surface_legends,
            0
        );
    }

    #[test]
    fn the_statistics_say_what_was_stroked() {
        let set = styles();
        let store = store_in("Triangles", true);
        // At 8 px a unit the sides are 80 px and the diagonal 113: all five edges stroked, the
        // shared diagonal once.
        let (_, stats) = paint(Some(&store), &set, &PlanPaintOptions::default());
        assert_eq!(stats.surface_edges_stroked, 5);
        assert_eq!(stats.surface_triangles_tinted, 0);
    }

    #[test]
    fn contours_are_traced_once_across_paints_through_one_cache() {
        let set = styles();
        let store = store_in("Contours", true);
        let options = PlanPaintOptions::default();
        let mut cache = PlanPaintCache::default();
        let _ = paint_with(Some(&store), &set, &options, &mut cache);
        let _ = paint_with(Some(&store), &set, &options, &mut cache);
        assert_eq!(cache.surface_cache().contour_tracings(), 1);
        assert_eq!(cache.surface_cache().surface_count(), 1);
        // A surface gone from the drawing is let go at the next paint.
        let none = SurfaceStore::new();
        let _ = paint_with(Some(&none), &set, &options, &mut cache);
        assert_eq!(cache.surface_cache().surface_count(), 0);
    }

    #[test]
    fn the_documents_surfaces_and_styles_are_what_plan_source_of_hands() {
        let mut document = Document::default();
        document
            .set_surfaces(store_in("", true), "SET_SURFACES")
            .expect("the surfaces are set");
        let source = plan_source_of(&document);
        assert!(
            source
                .surfaces
                .is_some_and(|surfaces| std::ptr::eq(surfaces, document.surfaces()))
        );
        assert!(
            source
                .surface_styles
                .is_some_and(|styles| std::ptr::eq(styles, document.surface_styles()))
        );
    }

    #[test]
    fn a_smooth_legend_places_each_stop_at_its_value_not_at_its_place_in_the_list() {
        // The built-in Cut Fill style: stops at -2, -1, -0.5, 0, 0.5, 1 and 2, not evenly spaced,
        // drawn as one bar labelled -2 at the foot and 2 at the head. A stop is at (value + 2) / 4
        // of the way up: -1 a quarter, 0 half, +1 three quarters. Placed by its place in the list
        // (i / 6) the -1 stop was at 0.167 and the +1 at 0.833: a reader of the labelled ends took
        // a 1 m fill (#EF8A62) for 1.33 m.
        let set = built_in_surface_styles();
        let cut_fill = set.find("Cut Fill").expect("the built-in Cut Fill style");
        let colouring = surface_colouring(cut_fill, SurfaceFaces::Elevation, -2.0, 2.0);
        let legend = surface_legend(&colouring);
        assert!(legend.gradient);
        assert_eq!(legend.entries.len(), 7);

        let font = Font::sized(FontSize::Pixels(11));
        let mut image = Image::new(240, 240);
        image.fill(Color::BLACK);
        let mut panel = RectF::default();
        paint_on(&mut image, |canvas| {
            panel = paint_surface_legend(
                canvas,
                &legend,
                "diff - Cut Fill",
                PointF::new(230.0, 10.0),
                &font,
                Color::WHITE,
                Color::rgb(0, 0, 0),
            );
        });
        // The bar, as paint_surface_legend lays it: 6 px in from the panel's left, below the
        // caption and the title and 4 px; six lines tall; as wide as a swatch.
        let line = metrics().height(&font);
        let swatch = katana_math::cpp::max(10.0, line * 0.8);
        let bar_top = panel.top() + 6.0 + 2.0 * line + 4.0;
        let bar_height = 6.0 * line;
        let x = (panel.left() + 6.0 + 0.5 * swatch) as u32;
        // The pixel row whose centre is nearest `fraction` of the way up the bar.
        let at = |fraction: f64| {
            ink_at(
                &image,
                x,
                (bar_top + bar_height - fraction * bar_height).floor() as u32,
            )
        };
        // The stops' own colours (the style's, ColorBrewer's seven-class RdBu): -1 #67A9CF, 0
        // #F7F7F7, +1 #EF8A62. A pixel centre is within half a pixel of the stop's place and the
        // gradient moves about ten levels a pixel here, so fourteen levels is the width of "at" -
        // and the old place, a sixth of the bar from the stop, was fifty levels off.
        assert!(
            channel_gap(at(0.25), (0x67, 0xA9, 0xCF)) <= 14,
            "{:?}",
            at(0.25)
        );
        assert!(
            channel_gap(at(0.50), (0xF7, 0xF7, 0xF7)) <= 14,
            "{:?}",
            at(0.50)
        );
        assert!(
            channel_gap(at(0.75), (0xEF, 0x8A, 0x62)) <= 14,
            "{:?}",
            at(0.75)
        );
        // The ends: -2 #2166AC at the foot, +2 #B2182B at the head.
        assert!(
            channel_gap(at(0.01), (0x21, 0x66, 0xAC)) <= 14,
            "{:?}",
            at(0.01)
        );
        assert!(
            channel_gap(at(0.99), (0xB2, 0x18, 0x2B)) <= 14,
            "{:?}",
            at(0.99)
        );
    }

    #[test]
    fn a_legend_too_tall_for_the_view_below_the_ones_already_drawn_is_left_out_not_cut_off() {
        // Three surfaces in a style with a five-band legend of height h, stacked down from the
        // top-right corner with 10 px between them. The view is 2 h + 50 tall: the first legend
        // ends at 10 + h, the second starts at 20 + h and ends at 20 + 2 h, and a third would
        // start at 30 + 2 h inside the view (its foot margin is at 2 h + 40) and end at 30 + 3 h,
        // far past it. It was drawn, counted and its lowest rows lost.
        let set = styles();
        let mut store = SurfaceStore::new();
        for name in ["a", "b", "c"] {
            store
                .add(NamedSurface {
                    name: String::from(name),
                    surface: Some(square()),
                    source: String::from("test"),
                    style: String::from("Bands"),
                    visible: true,
                })
                .expect("the surface is added");
        }
        let font = Font::family(
            &PlanPaintOptions::default().font_family,
            FontSize::Pixels(11),
        );
        let bands = set.find("Bands").expect("the Bands style");
        let colouring = surface_colouring(bands, SurfaceFaces::Elevation, 0.0, 10.0);
        let h = measure_surface_legend(
            &surface_legend(&colouring),
            "a - Bands",
            &font,
            metrics().as_ref(),
        )
        .height;
        assert!(h > 40.0, "{h}");
        let height = 2.0 * h + 50.0;
        let mut f = PlanFrame::default();
        f.transform.resize(300.0, height);
        f.transform.scale = 8.0;
        f.transform.center = Point2::new(5.0, 5.0);
        let empty = Model::new();
        let source = PlanSource {
            model: Some(&empty),
            surfaces: Some(&store),
            surface_styles: Some(&set),
            ..PlanSource::default()
        };
        let mut image = Image::new(300, height as u32);
        image.fill(BLACK);
        let mut cache = PlanPaintCache::default();
        let options = PlanPaintOptions::default();
        let mut stats = PlanPaintStats::default();
        paint_on(&mut image, |canvas| {
            stats = paint_plan(canvas, &source, &f, &options, &mut cache);
        });
        assert_eq!(stats.surfaces_drawn, 3);
        assert_eq!(stats.surface_legends, 2);
    }

    #[test]
    fn a_style_whose_contours_are_refused_says_why_in_the_statistics_not_nowhere() {
        // An interval of a millionth over 10 units of relief crosses each triangle ten million
        // times, past the four million points the plan allows: the contours are not drawn, and the
        // paint's statistics say so - the plan view logs it once, the headless report prints it.
        let mut set = styles();
        let mut fine = bare("Fine");
        fine.contours.show = true;
        fine.contours.interval = 1.0e-6;
        set.styles.push(fine);
        let refused = store_in("Fine", true);
        let (_, stats) = paint(Some(&refused), &set, &PlanPaintOptions::default());
        assert_eq!(stats.surface_problems.len(), 1);
        // Whose it is - the surface and its style - and why; an interval of a millionth is
        // written to six decimals, 0.000001.
        let problem = &stats.surface_problems[0];
        assert_eq!(problem.surface, "ground");
        assert_eq!(problem.style, "Fine");
        assert!(
            problem.reason.contains("contours: an interval of 0.000001"),
            "{}",
            problem.reason
        );
        assert!(
            problem.text().starts_with("ground (style Fine): contours"),
            "{}",
            problem.text()
        );
        // A style whose contours are drawn says nothing.
        let drawn = store_in("Contours", true);
        assert!(
            paint(Some(&drawn), &set, &PlanPaintOptions::default())
                .1
                .surface_problems
                .is_empty()
        );
    }
}
