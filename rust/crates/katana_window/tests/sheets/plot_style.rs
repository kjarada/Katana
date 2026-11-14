//! The twins of tests/qt_widgets/plotting/test_plot_style.cpp: the plot style on the canvas
//! (katana_window::plotting::plot_style) and on a painted sheet - every colour the sheet painter
//! puts on paper follows the colour mode and every line the line weight scale (docs/plotting.md,
//! "Plot styles and output"). A sheet is painted in part, at the resolution a line's width needs.
//!
//! One assertion of the C++ has no counterpart: a greyscale raster keeping its image's dots per
//! metre. The canvas Image keeps no resolution (the writers are given it), as plot_style says.

use katana_cad::plot::{PlotColourMode, PlotSettings, luminance};
use katana_cad::plotting::frame::built_in_frame;
use katana_cad::plotting::sheet_set::{GridStyle, Sheet, SheetSet, Viewport, ViewportKind};
use katana_cad::scene::SceneSurface;
use katana_entity::{
    Alignment, Color as EntityColor, Entity, Geometry, HatchPattern, Layer, Model,
};
use katana_geometry::{
    AlignmentPi, Box2, HorizontalAlignment, Point2, Point3, Polyline2, Segment2,
};
use katana_interop::reference_data::{RasterOverlay, ReferenceData};
use katana_terrain::tin_surface::TinSurface;
use katana_window::canvas::{Color, Image, ImageFormat, PointF};
use katana_window::plan_painter::PlanSource;
use katana_window::plotting::plot_style::{greyscale_raster, plot_fill, plot_image, plot_ink};
use katana_window::sheet_painter::{
    SheetPaintCache, SheetPaintOptions, SheetSource, paint_sheet,
};

use crate::support::{GREY, bx, keep, near, paint_on};

/// A3 landscape.
const SHEET_HEIGHT_MM: f64 = 297.0;

fn styled(mode: PlotColourMode, line_weight_scale: f64) -> PlotSettings {
    PlotSettings {
        colour_mode: mode,
        line_weight_scale,
        ..PlotSettings::default()
    }
}

fn mode_of(mode: PlotColourMode) -> PlotSettings {
    styled(mode, 1.0)
}

fn add_line(model: &mut Model, a: Point2, b: Point2, colour: Option<EntityColor>) {
    model
        .entities
        .add(Entity {
            geometry: Geometry::Line(Segment2::new(a, b)),
            layer: String::from("0"),
            color: colour,
            ..Entity::default()
        })
        .expect("the line is added");
}

fn rgb(r: u8, g: u8, b: u8) -> EntityColor {
    EntityColor { r, g, b, a: 255 }
}

/// An A3 sheet, the built-in frame, one plan viewport at 1 : 1000 centred on the origin over
/// 100..300 x 100..250 mm of paper.
fn plan_sheet() -> SheetSet {
    SheetSet {
        sheets: vec![Sheet {
            id: String::from("s1"),
            name: String::from("STYLE"),
            viewports: vec![Viewport {
                id: String::from("vp1"),
                kind: ViewportKind::Plan,
                rect: bx(100.0, 100.0, 300.0, 250.0),
                scale: 1000.0,
                centre: Point2::new(0.0, 0.0),
                ..Viewport::default()
            }],
            ..Sheet::default()
        }],
        ..SheetSet::default()
    }
}

fn source_of(model: &Model) -> SheetSource<'_> {
    SheetSource {
        plan: PlanSource {
            model: Some(model),
            ..PlanSource::default()
        },
        ..SheetSource::default()
    }
}

/// Paints sheet 0 of `set` in `plot`, only the paper box `region`, at `ppmm` device pixels a
/// millimetre: the painter's own origin shift, as the editor scrolls, so a fine resolution costs
/// only the pixels looked at.
fn painted_region(
    set: &SheetSet,
    source: &SheetSource<'_>,
    plot: &PlotSettings,
    region: &Box2,
    ppmm: f64,
) -> Image {
    let mut image = Image::new(
        (region.width() * ppmm).ceil() as u32,
        (region.height() * ppmm).ceil() as u32,
    );
    image.fill(GREY); // the painter's paper covers it
    let options = SheetPaintOptions {
        pixels_per_millimetre: ppmm,
        origin: PointF::new(
            -region.min.x * ppmm,
            -(SHEET_HEIGHT_MM - region.max.y) * ppmm,
        ),
        plot: *plot,
        ..SheetPaintOptions::default()
    };
    let mut cache = SheetPaintCache::new();
    paint_on(&mut image, |canvas| {
        let _ = paint_sheet(canvas, set, 0, source, &options, &mut cache);
    });
    image
}

fn painted_sheet(
    set: &SheetSet,
    source: &SheetSource<'_>,
    plot: &PlotSettings,
    name: &str,
) -> Image {
    let image = painted_region(
        set,
        source,
        plot,
        &bx(0.0, 0.0, 420.0, SHEET_HEIGHT_MM),
        4.0,
    );
    keep(&image, name);
    image
}

/// The pixel of paper point `p` on an image painted of `region` at `ppmm`.
fn pixel_of(p: Point2, region: &Box2, ppmm: f64) -> (i32, i32) {
    (
        ((p.x - region.min.x) * ppmm).floor() as i32,
        ((region.max.y - p.y) * ppmm).floor() as i32,
    )
}

fn chroma(pixel: Color) -> i32 {
    let high = pixel.r.max(pixel.g).max(pixel.b);
    let low = pixel.r.min(pixel.g).min(pixel.b);
    i32::from(high) - i32::from(low)
}

fn lightness(pixel: Color) -> i32 {
    i32::from(pixel.r) + i32::from(pixel.g) + i32::from(pixel.b)
}

fn brightest_channel(pixel: Color) -> u8 {
    pixel.r.max(pixel.g).max(pixel.b)
}

/// The darkest pixel within `radius_mm` of paper point `p`.
fn darkest_near(image: &Image, p: Point2, region: &Box2, ppmm: f64, radius_mm: f64) -> Color {
    let a = pixel_of(
        Point2::new(p.x - radius_mm, p.y + radius_mm),
        region,
        ppmm,
    );
    let b = pixel_of(
        Point2::new(p.x + radius_mm, p.y - radius_mm),
        region,
        ppmm,
    );
    let mut darkest = Color::WHITE;
    for y in a.1.max(0)..=b.1.min(image.height() as i32 - 1) {
        for x in a.0.max(0)..=b.0.min(image.width() as i32 - 1) {
            let pixel = image.pixel(x as u32, y as u32);
            if lightness(pixel) < lightness(darkest) {
                darkest = pixel;
            }
        }
    }
    darkest
}

/// The most colourful pixel of the image, and how many have any colour.
struct Colourfulness {
    most: i32,
    coloured: usize,
}

fn colourfulness(image: &Image, threshold: i32) -> Colourfulness {
    let mut out = Colourfulness {
        most: 0,
        coloured: 0,
    };
    for y in 0..image.height() {
        for x in 0..image.width() {
            let c = chroma(image.pixel(x, y));
            out.most = out.most.max(c);
            if c > threshold {
                out.coloured += 1;
            }
        }
    }
    out
}

fn is_dark(pixel: Color) -> bool {
    pixel.r < 128 && pixel.g < 128 && pixel.b < 128
}

/// Dark pixels (every channel under 128) along the device row through paper height `y`, between
/// paper x `x0` and `x1`: a vertical line's width.
fn dark_across(image: &Image, y: f64, x0: f64, x1: f64, region: &Box2, ppmm: f64) -> i32 {
    let row = pixel_of(Point2::new(x0, y), region, ppmm).1;
    let mut count = 0;
    let first = pixel_of(Point2::new(x0, y), region, ppmm).0;
    let last = pixel_of(Point2::new(x1, y), region, ppmm).0;
    for x in first..=last {
        if is_dark(image.pixel(x as u32, row as u32)) {
            count += 1;
        }
    }
    count
}

/// The same down the device column through paper x `x`: a level line's width.
fn dark_down(image: &Image, x: f64, y0: f64, y1: f64, region: &Box2, ppmm: f64) -> i32 {
    let column = pixel_of(Point2::new(x, y0), region, ppmm).0;
    let mut count = 0;
    let first = pixel_of(Point2::new(x, y1), region, ppmm).1;
    let last = pixel_of(Point2::new(x, y0), region, ppmm).1;
    for y in first..=last {
        if is_dark(image.pixel(column as u32, y as u32)) {
            count += 1;
        }
    }
    count
}

fn red_logo() -> Image {
    let mut logo = Image::new(400, 100);
    logo.fill(Color::rgb(220, 0, 0));
    logo
}

mod plot_style {
    use super::*;

    #[test]
    fn ink_and_fills_follow_the_colour_rules() {
        let colour = mode_of(PlotColourMode::Colour);
        let grey = mode_of(PlotColourMode::Greyscale);
        let mono = mode_of(PlotColourMode::Monochrome);
        // Colour keeps a colour; white prints black in every mode.
        assert_eq!(
            plot_ink(Color::rgb(0, 147, 208), &colour),
            Color::rgb(0, 147, 208)
        );
        assert_eq!(
            plot_ink(Color::rgb(255, 255, 255), &colour),
            Color::rgb(0, 0, 0)
        );
        // Greyscale: the luminance, alpha kept.
        assert_eq!(
            plot_ink(Color::rgba(0, 0, 255, 99), &grey),
            Color::rgba(29, 29, 29, 99)
        );
        assert_eq!(
            plot_fill(Color::rgba(0, 0, 255, 99), &grey),
            Color::rgba(29, 29, 29, 99)
        );
        // Monochrome: every pen black however light; a fill black or white by its luminance
        // against 128.
        assert_eq!(
            plot_ink(Color::rgb(255, 255, 0), &mono),
            Color::rgb(0, 0, 0)
        );
        assert_eq!(
            plot_fill(Color::rgba(200, 0, 30, 18), &mono),
            Color::rgba(0, 0, 0, 18)
        );
        assert_eq!(
            plot_fill(Color::rgb(255, 255, 0), &mono),
            Color::rgb(255, 255, 255)
        );
    }

    #[test]
    fn images_print_as_the_grey_of_their_luminance_with_their_alpha() {
        let mut image = Image::new(3, 1);
        image.set_pixel(0, 0, Color::rgba(255, 0, 0, 255));
        image.set_pixel(1, 0, Color::rgba(0, 0, 255, 0)); // transparent ground
        image.set_pixel(2, 0, Color::rgba(10, 200, 30, 128));
        assert!(plot_image(&image, &mode_of(PlotColourMode::Colour)) == image);
        for mode in [PlotColourMode::Greyscale, PlotColourMode::Monochrome] {
            let grey = plot_image(&image, &mode_of(mode));
            assert_eq!(grey.pixel(0, 0), Color::rgba(76, 76, 76, 255));
            assert_eq!(grey.pixel(1, 0).a, 0);
            // 2.99 + 117.4 + 3.42
            assert_eq!(grey.pixel(2, 0), Color::rgba(124, 124, 124, 128));
        }
        assert!(plot_image(&Image::default(), &mode_of(PlotColourMode::Greyscale)).is_null());
    }

    #[test]
    fn a_greyscale_raster_is_one_channel_of_the_luminance() {
        let mut image = Image::new(2, 1);
        image.set_pixel(0, 0, Color::rgb(255, 0, 0));
        image.set_pixel(1, 0, Color::rgb(255, 255, 255));
        let grey = greyscale_raster(&image);
        assert_eq!(grey.format(), ImageFormat::Grey8);
        assert_eq!(grey.bytes()[0], 76);
        assert_eq!(grey.bytes()[1], 255);
        // The C++ also holds that the raster keeps its image's 11811 dots a metre: an Image keeps
        // no resolution (see the header).
    }

    #[test]
    fn greyscale_and_monochrome_leave_no_colour_anywhere_on_the_sheet() {
        // Colour from every source the sheet painter has: the frame's coloured legend lines,
        // glyphs and texts; a plan's red and green lines; the logo; a cross section's coloured
        // ground.
        let ground = TinSurface::create(
            vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(100.0, 0.0, 50.0),
                Point3::new(100.0, 100.0, 50.0),
                Point3::new(0.0, 100.0, 0.0),
            ],
            vec![[0, 1, 2], [0, 2, 3]],
            Vec::new(),
        )
        .expect("the ground is a surface");
        let mut model = Model::new();
        add_line(
            &mut model,
            Point2::new(-80.0, 0.0),
            Point2::new(80.0, 0.0),
            Some(rgb(255, 0, 0)),
        );
        add_line(
            &mut model,
            Point2::new(0.0, -60.0),
            Point2::new(0.0, 60.0),
            Some(rgb(0, 160, 40)),
        );
        let pi = |y: f64| AlignmentPi {
            point: Point2::new(50.0, y),
            ..AlignmentPi::default()
        };
        model
            .alignments
            .add(Alignment {
                name: String::from("ROAD"),
                horizontal: HorizontalAlignment {
                    pis: vec![pi(10.0), pi(90.0)],
                    ..HorizontalAlignment::default()
                },
                ..Alignment::default()
            })
            .expect("the road is added");

        let mut set = plan_sheet();
        let mut across = Viewport {
            id: String::from("vp2"),
            kind: ViewportKind::CrossSections,
            rect: bx(305.0, 100.0, 405.0, 250.0),
            scale: 250.0,
            auto_centre: true,
            ..Viewport::default()
        };
        across.source.alignment = String::from("ROAD");
        across.source.stations = vec![40.0];
        across.source.section_half_width = 20.0;
        set.sheets[0].viewports.push(across);

        let mut source = source_of(&model);
        source.surfaces.push(SceneSurface {
            name: String::from("GROUND"),
            surface: Some(&ground),
            ..SceneSurface::default()
        });
        source.logo = red_logo();

        let colour = painted_sheet(
            &set,
            &source,
            &mode_of(PlotColourMode::Colour),
            "no_colour_anywhere_colour",
        );
        // The colour is there to lose: thousands of strongly coloured pixels.
        assert!(colourfulness(&colour, 60).coloured > 5000);

        for mode in [PlotColourMode::Greyscale, PlotColourMode::Monochrome] {
            let sheet = painted_sheet(
                &set,
                &source,
                &mode_of(mode),
                if mode == PlotColourMode::Greyscale {
                    "no_colour_anywhere_grey"
                } else {
                    "no_colour_anywhere_mono"
                },
            );
            let found = colourfulness(&sheet, 2);
            assert!(found.most <= 2, "{mode:?}: {}", found.most);
            assert_eq!(found.coloured, 0, "{mode:?}");
        }
    }

    // What the other sheet features draw follows the style too: an aerial photo cropped under a
    // plan, the plan's coordinate grid, a key plan's tinted outlines, and a drawing register's
    // highlighted row. None of it leaves a coloured pixel in greyscale or monochrome.
    #[test]
    fn imagery_grids_key_plans_and_tables_follow_the_style() {
        let mut rgba = Vec::with_capacity(40 * 40 * 4);
        for _ in 0..40 * 40 {
            rgba.extend_from_slice(&[40, 110, 200, 255]); // a strong blue
        }
        let photo = RasterOverlay {
            name: String::from("ortho"),
            width: 40,
            height: 40,
            rgba,
            geotransform: [-200.0, 10.0, 0.0, 200.0, 0.0, -10.0],
            has_geotransform: true,
            ..RasterOverlay::default()
        };
        let mut reference = ReferenceData::new();
        let _ = reference.add(photo);
        let mut model = Model::new();
        add_line(
            &mut model,
            Point2::new(-80.0, 0.0),
            Point2::new(80.0, 0.0),
            Some(rgb(255, 0, 0)),
        );

        let mut set = plan_sheet();
        set.sheets[0].viewports[0].grid_style = GridStyle::Lines;
        set.sheets[0].viewports.push(Viewport {
            id: String::from("vp2"),
            kind: ViewportKind::KeyPlan,
            rect: bx(305.0, 180.0, 405.0, 250.0),
            auto_scale: true,
            auto_centre: true,
            ..Viewport::default()
        });
        set.sheets[0].viewports.push(Viewport {
            id: String::from("vp3"),
            kind: ViewportKind::SheetIndex,
            rect: bx(305.0, 100.0, 405.0, 175.0),
            ..Viewport::default()
        });
        let mut second = set.sheets[0].clone();
        second.id = String::from("s2");
        second.name = String::from("NEXT");
        second.viewports = vec![set.sheets[0].viewports[0].clone()];
        second.viewports[0].id = String::from("vp4");
        second.viewports[0].centre = Point2::new(150.0, 0.0);
        set.sheets.push(second);

        let mut source = source_of(&model);
        source.plan.reference = Some(&reference);
        let colour = painted_sheet(
            &set,
            &source,
            &mode_of(PlotColourMode::Colour),
            "features_follow_the_style",
        );
        assert!(colourfulness(&colour, 60).coloured > 5000);
        for mode in [PlotColourMode::Greyscale, PlotColourMode::Monochrome] {
            let sheet = painted_sheet(
                &set,
                &source,
                &mode_of(mode),
                if mode == PlotColourMode::Greyscale {
                    "features_follow_the_style_grey"
                } else {
                    "features_follow_the_style_mono"
                },
            );
            let found = colourfulness(&sheet, 2);
            assert!(found.most <= 2, "{mode:?}: {}", found.most);
            assert_eq!(found.coloured, 0, "{mode:?}");
        }
    }

    #[test]
    fn monochrome_prints_every_frame_line_black_and_greyscale_as_its_luminance() {
        let model = Model::new();
        let mut set = plan_sheet();
        set.sheets[0].viewports.clear();
        let source = source_of(&model);
        let frame = built_in_frame().as_ref().expect("the built-in frame reads");
        // The title block, where the frame's coloured legend lines are, at 10 pixels a millimetre:
        // a 0.18 mm line is 1.8 pixels, so one pixel of it is nearly all ink.
        let region = bx(20.0, 12.0, 412.0, 36.0);
        let ppmm = 10.0;
        let colour = painted_region(
            &set,
            &source,
            &mode_of(PlotColourMode::Colour),
            &region,
            ppmm,
        );
        let grey = painted_region(
            &set,
            &source,
            &mode_of(PlotColourMode::Greyscale),
            &region,
            ppmm,
        );
        let mono = painted_region(
            &set,
            &source,
            &mode_of(PlotColourMode::Monochrome),
            &region,
            ppmm,
        );

        let mut coloured = 0;
        let mut checked = 0;
        for line in &frame.polylines {
            // Solid lines of 0.15 mm and more: a dashed one's middle may fall in a gap, and the
            // 0.13 mm legend grid is under a pixel and a half.
            if !line.plots || line.points.len() < 2 || line.weight_mm < 0.15 || line.dash_mm > 0.0
            {
                continue;
            }
            let middle = Point2::new(
                (line.points[0].x + line.points[1].x) / 2.0,
                (line.points[0].y + line.points[1].y) / 2.0,
            );
            if !region.contains(middle) {
                continue;
            }
            checked += 1;
            // Monochrome: black, whatever the line's colour.
            let black = darkest_near(&mono, middle, &region, ppmm, 0.3);
            assert!(
                brightest_channel(black) <= 40,
                "a line at {}, {}",
                middle.x,
                middle.y
            );
            assert!(chroma(black) <= 2);
            let ink = line.colour;
            if ink.r == ink.g && ink.g == ink.b {
                continue;
            }
            coloured += 1;
            // Colour: the line's own colour.
            let own = darkest_near(&colour, middle, &region, ppmm, 0.3);
            assert!(chroma(own) > 60, "a line at {}, {}", middle.x, middle.y);
            // Greyscale: a grey as light as the colour (its luminance, lightened a little where
            // the line does not cover a whole pixel).
            let shade = darkest_near(&grey, middle, &region, ppmm, 0.3);
            let expected = i32::from(luminance(ink));
            assert!(chroma(shade) <= 2);
            assert!(
                i32::from(shade.r) >= expected - 3,
                "a line at {}, {}",
                middle.x,
                middle.y
            );
            assert!(
                i32::from(shade.r) <= expected + 40,
                "a line at {}, {}",
                middle.x,
                middle.y
            );
        }
        // The legend's blue, gold, yellow, orange and brown lines, and the rules.
        assert!(coloured >= 12, "{coloured}");
        assert!(checked >= 20, "{checked}");
    }

    #[test]
    fn a_plan_on_a_sheet_follows_the_style() {
        // A red line across the plan at y 175 on the paper.
        let mut model = Model::new();
        add_line(
            &mut model,
            Point2::new(-80.0, 0.0),
            Point2::new(80.0, 0.0),
            Some(rgb(255, 0, 0)),
        );
        let set = plan_sheet();
        let source = source_of(&model);
        let region = bx(190.0, 170.0, 210.0, 180.0);
        let ppmm = 10.0;
        let on_line = Point2::new(200.0, 175.0);
        let darkest_in = |mode: PlotColourMode| {
            darkest_near(
                &painted_region(&set, &source, &mode_of(mode), &region, ppmm),
                on_line,
                &region,
                ppmm,
                0.5,
            )
        };

        let red = darkest_in(PlotColourMode::Colour);
        assert!(red.r > 200, "{red:?}");
        assert!(red.g < 60, "{red:?}");

        let grey = darkest_in(PlotColourMode::Greyscale);
        assert!(chroma(grey) <= 2, "{grey:?}");
        assert!(near(f64::from(grey.r), 76.0, 12.0), "{grey:?}"); // red's luminance

        let black = darkest_in(PlotColourMode::Monochrome);
        assert!(brightest_channel(black) <= 20, "{black:?}");
    }

    #[test]
    fn a_plans_solid_fill_is_greyed_once_and_in_monochrome_a_light_one_drops_out() {
        // Two 40 m squares hatched solid, a light yellow one left of the centre and a dark blue
        // one right of it: at 1 : 1000 their middles are paper (160, 175) and (240, 175).
        let mut model = Model::new();
        model
            .hatch_patterns
            .add(HatchPattern {
                name: String::from("fill"),
                solid: true,
                ..HatchPattern::default()
            })
            .expect("the fill is added");
        let light = rgb(255, 230, 0); // luminance 211
        let dark = rgb(0, 0, 200); // luminance 23
        for (name, colour, x0) in [("light", light, -60.0), ("dark", dark, 20.0)] {
            model
                .layers
                .add(Layer {
                    name: String::from(name),
                    color: colour,
                    hatch_pattern: String::from("fill"),
                    ..Layer::default()
                })
                .expect("the layer is added");
            model
                .entities
                .add(Entity {
                    geometry: Geometry::Polyline(Polyline2::new(
                        vec![
                            Point2::new(x0, -20.0),
                            Point2::new(x0 + 40.0, -20.0),
                            Point2::new(x0 + 40.0, 20.0),
                            Point2::new(x0, 20.0),
                        ],
                        true,
                    )),
                    layer: String::from(name),
                    ..Entity::default()
                })
                .expect("the square is added");
        }
        let set = plan_sheet();
        let source = source_of(&model);
        let region = bx(130.0, 150.0, 270.0, 200.0);
        let ppmm = 10.0; // the outline's 0.25 mm is 2.5 pixels
        let middle_of = |image: &Image, x: f64| {
            let at = pixel_of(Point2::new(x, 175.0), &region, ppmm);
            image.pixel(at.0 as u32, at.1 as u32)
        };

        // Greyscale: each fill the grey of its own luminance - not darkened by a second pass.
        let grey = painted_region(
            &set,
            &source,
            &mode_of(PlotColourMode::Greyscale),
            &region,
            ppmm,
        );
        assert_eq!(middle_of(&grey, 160.0), Color::rgb(211, 211, 211));
        assert_eq!(middle_of(&grey, 240.0), Color::rgb(23, 23, 23));

        // Monochrome: the light fill drops out to the paper's white, leaving its black outline;
        // the dark one prints solid black.
        let mono = painted_region(
            &set,
            &source,
            &mode_of(PlotColourMode::Monochrome),
            &region,
            ppmm,
        );
        assert_eq!(middle_of(&mono, 160.0), Color::rgb(255, 255, 255));
        assert_eq!(middle_of(&mono, 240.0), Color::rgb(0, 0, 0));
        let edge = darkest_near(&mono, Point2::new(140.0, 175.0), &region, ppmm, 0.5);
        assert!(brightest_channel(edge) <= 40, "{edge:?}");
    }

    #[test]
    fn the_logo_prints_grey_in_greyscale_and_monochrome() {
        let model = Model::new();
        let set = plan_sheet();
        let mut source = source_of(&model);
        source.logo = red_logo();
        let middle = built_in_frame()
            .as_ref()
            .expect("the built-in frame reads")
            .cell("logo")
            .expect("the frame has a logo slot")
            .rect
            .center();
        let region = bx(
            middle.x - 5.0,
            middle.y - 2.0,
            middle.x + 5.0,
            middle.y + 2.0,
        );
        for mode in [PlotColourMode::Greyscale, PlotColourMode::Monochrome] {
            let image = painted_region(&set, &source, &mode_of(mode), &region, 4.0);
            let pixel = image.pixel(image.width() / 2, image.height() / 2);
            // 220 red is a luminance of 66: grey, not thresholded to black.
            assert!(near(f64::from(pixel.r), 66.0, 2.0), "{mode:?}: {pixel:?}");
            assert!(chroma(pixel) <= 1, "{mode:?}: {pixel:?}");
        }
    }

    #[test]
    fn the_line_weight_scale_thickens_the_frame_and_the_plan() {
        let mut model = Model::new();
        // Layer 0: 0.25 mm.
        add_line(
            &mut model,
            Point2::new(-80.0, 0.0),
            Point2::new(80.0, 0.0),
            None,
        );
        let set = plan_sheet();
        let source = source_of(&model);
        let ppmm = 10.0;
        // The border's left rule (0.53 mm at x 21.825) and the plan's line (at y 175 on the
        // paper).
        let border = bx(18.0, 150.0, 26.0, 152.0);
        let plan = bx(195.0, 170.0, 205.0, 180.0);
        let widths = |scale: f64| {
            let plot = styled(PlotColourMode::Colour, scale);
            let left = painted_region(&set, &source, &plot, &border, ppmm);
            let line = painted_region(&set, &source, &plot, &plan, ppmm);
            (
                f64::from(dark_across(&left, 151.0, 18.5, 25.5, &border, ppmm)),
                f64::from(dark_down(&line, 200.0, 170.5, 179.5, &plan, ppmm)),
            )
        };
        let (rule, line) = widths(1.0);
        assert!(near(rule, 5.0, 1.0), "{rule}"); // 0.53 mm
        assert!(near(line, 2.5, 1.0), "{line}"); // 0.25 mm
        let (rule_bold, line_bold) = widths(2.0);
        assert!(near(rule_bold, 11.0, 1.0), "{rule_bold}");
        assert!(near(line_bold, 5.0, 1.0), "{line_bold}");
        let (rule_fine, line_fine) = widths(0.5);
        assert!(near(rule_fine, 2.5, 1.0), "{rule_fine}");
        assert!(line_fine <= 2.0, "{line_fine}");
        assert!(line_fine >= 1.0, "{line_fine}");
    }
}
