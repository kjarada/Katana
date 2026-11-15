//! The twins of tests/qt_widgets/test_sheet_painter.cpp: the sheet painter
//! (katana_window::sheet_painter) - the frame, its fields and logo slot, each viewport kind, and
//! the multi-page PDF. Painted into images at 4 px a millimetre and read back pixel by pixel.
//!
//! The PDFs a case reads are written readable (plot_sheets_to_readable_pdf), as the C++ read
//! QPdfWriter's; the sizes it compares are of files written as a plot writes them.

use std::path::Path;
use std::sync::Arc;

use katana_cad::plot::{PaperSize, paper_dimensions};
use katana_cad::plotting::frame::built_in_frame;
use katana_cad::plotting::sheet_set::{Sheet, SheetSet, Viewport, ViewportKind};
use katana_cad::scene::SceneSurface;
use katana_cad::terrain::surface_style::{
    SurfaceDisplayStyle, SurfaceStyleSet, built_in_surface_styles,
};
use katana_entity::{Alignment, Model};
use katana_geometry::{AlignmentPi, Box2, HorizontalAlignment, Point2, Point3};
use katana_interop::reference_data::{RasterOverlay, ReferenceData};
use katana_math::numerics::PI;
use katana_terrain::surface_store::{NamedSurface, SurfaceStore};
use katana_terrain::tin_surface::TinSurface;
use katana_window::canvas::{Color, Image, PdfCanvas, PdfDocument};
use katana_window::plan_painter::PlanSource;
use katana_window::sheet_painter::{
    SheetPaintCache, SheetPaintOptions, SheetSource, paint_sheet, plot_sheets_to_pdf,
    plot_sheets_to_readable_pdf, resolve_plan_viewport,
};

use crate::support::{
    Painting, add_line, bx, colour_in, double_eq, ink_in, near, painted, painted_with,
};
use crate::window_support::Scratch;

fn a3(name: &str) -> Sheet {
    Sheet {
        id: String::from("s1"),
        name: String::from(name),
        ..Sheet::default()
    }
}

fn plan_at(rect: Box2, scale: f64, centre: Point2, rotation: f64) -> Viewport {
    Viewport {
        id: String::from("vp1"),
        kind: ViewportKind::Plan,
        rect,
        scale,
        centre,
        rotation,
        ..Viewport::default()
    }
}

fn one_sheet() -> SheetSet {
    SheetSet {
        sheets: vec![a3("PLAN")],
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

/// Pixels in the paper box close to `colour`: every channel less than 40 from it.
fn colour_near(image: &Image, paper: &Box2, colour: Color) -> usize {
    colour_in(image, paper, colour, 39)
}

/// A `width` x `height` raster whose pixel (x, y) is `colour(x, y)` (with its alpha), placed by
/// `geotransform`.
fn raster_of(
    width: i32,
    height: i32,
    geotransform: [f64; 6],
    colour: impl Fn(i32, i32) -> Color,
) -> RasterOverlay {
    let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
    for y in 0..height {
        for x in 0..width {
            let c = colour(x, y);
            rgba.extend_from_slice(&[c.r, c.g, c.b, c.a]);
        }
    }
    RasterOverlay {
        name: String::from("ortho"),
        width,
        height,
        rgba,
        geotransform,
        has_geotransform: true,
        ..RasterOverlay::default()
    }
}

/// The set's first sheet as a one-page PDF at `dpi`, as plot_sheets_to_pdf writes it but with the
/// imagery capped at `raster_dpi_cap` (0: not capped below the plot's own resolution). Returns the
/// file's size in bytes.
fn sheet_pdf_bytes(
    path: &Path,
    set: &SheetSet,
    source: &SheetSource<'_>,
    dpi: f64,
    raster_dpi_cap: f64,
) -> u64 {
    let sheet = &set.sheets[0];
    let paper = paper_dimensions(sheet.paper, sheet.landscape);
    let points = |millimetres: f64| millimetres * 72.0 / 25.4;
    let mut canvas = PdfCanvas::new(
        points(paper.width_mm),
        points(paper.height_mm),
        f64::from(dpi as i32),
    );
    let mut options = SheetPaintOptions {
        pixels_per_millimetre: dpi / 25.4,
        raster_dpi_cap,
        ..SheetPaintOptions::default()
    };
    options.plot.dpi = dpi;
    let mut cache = SheetPaintCache::new();
    let _ = paint_sheet(&mut canvas, set, 0, source, &options, &mut cache);
    let mut document = PdfDocument::new();
    document.add_page(canvas).expect("the page is added");
    document.write(path).expect("the PDF is written");
    std::fs::metadata(path).expect("the PDF is there").len()
}

/// How often `needle` is in `haystack`, and where.
fn places(haystack: &[u8], needle: &[u8]) -> Vec<usize> {
    haystack
        .windows(needle.len())
        .enumerate()
        .filter(|(_, window)| *window == needle)
        .map(|(at, _)| at)
        .collect()
}

// ---- a surface whose style cannot be drawn in whole ---------------------------------------------
//
// The plan painter refuses contours that would be traced more than four million times and reports
// it in its statistics (PlanPaintStats::surface_problems). A plot or a sheet paints the same plan,
// so the sheet's problems carry it - a plan viewport drawn without a surface's contours is paper
// wasted - where the editor shows it and every plot reports it (docs/surface_display.md, "In
// plan"; docs/plotting.md, "The sheet painter").

/// The 10 x 10 square z = x as the surface "ground", drawn in the style named. Each of its two
/// triangles runs from 0 to 10, so contours every millionth of a unit cross it ten million times a
/// triangle (past the limit of four million) and contours every unit cross it ten times.
fn ground_in(style: &str, visible: bool) -> SurfaceStore {
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
    let mut store = SurfaceStore::new();
    store
        .add(NamedSurface {
            name: String::from("ground"),
            surface: Some(Arc::new(tin)),
            source: String::from("test"),
            style: String::from(style),
            visible,
        })
        .expect("the surface is added");
    store
}

fn styles_with_fine() -> SurfaceStyleSet {
    let mut styles = built_in_surface_styles();
    let mut fine = SurfaceDisplayStyle {
        name: String::from("Fine"),
        ..SurfaceDisplayStyle::default()
    };
    fine.contours.show = true;
    fine.contours.interval = 1.0e-6;
    styles.styles.push(fine);
    styles
}

/// A sheet whose one plan, 1:1000 about `centre`, shows the surface when the centre is (5, 5).
fn plan_over(centre: Point2) -> SheetSet {
    let mut set = one_sheet();
    set.sheets[0].viewports.push(plan_at(
        bx(100.0, 100.0, 300.0, 250.0),
        1000.0,
        centre,
        0.0,
    ));
    set
}

mod sheet_painter {
    use super::*;

    #[test]
    fn the_frame_is_drawn_and_its_construction_guide_is_not() {
        let set = one_sheet();
        let model = Model::new();
        let source = source_of(&model);
        let (paper, _) = painted(&set, &source, "the_frame_is_drawn");
        // The border's left rule at x 21.825, halfway up the drawing area.
        assert!(ink_in(&paper, &bx(21.3, 150.0, 22.3, 151.0)) > 0);
        // The title block's top rule at y 34.625.
        assert!(ink_in(&paper, &bx(200.0, 34.1, 201.0, 35.1)) > 0);
        // The paper's edge: the construction guide does not plot.
        assert_eq!(ink_in(&paper, &bx(0.0, 100.0, 1.0, 200.0)), 0);

        let (editing, _) = painted_with(
            &set,
            &source,
            &Painting {
                construction: true,
                ..Painting::named("the_frame_is_drawn_editing")
            },
        );
        // Drawn while editing: a hairline centred on the edge, half of it on the paper.
        assert!(ink_in(&editing, &bx(0.0, 100.0, 1.0, 200.0)) > 0);
    }

    #[test]
    fn the_logo_fills_its_slot_and_the_slot_is_empty_without_one() {
        let set = one_sheet();
        let model = Model::new();
        let mut source = source_of(&model);
        let frame = built_in_frame().as_ref().expect("the built-in frame reads");
        let cell = frame.cell("logo").expect("the frame has a logo slot").rect;

        let (without, stats) = painted(&set, &source, "the_logo_without");
        assert!(!stats.logo_drawn);
        assert_eq!(ink_in(&without, &cell.inflated(-1.5)), 0);

        // A 4:1 red logo: drawn 40 x 10 mm in the middle of the cell.
        let red = Color::rgb(220, 0, 0);
        let mut logo = Image::new(400, 100);
        logo.fill(red);
        source.logo = logo;
        let (with, stats) = painted(&set, &source, "the_logo_with");
        assert!(stats.logo_drawn);
        let middle = cell.center();
        assert!(
            colour_near(
                &with,
                &bx(
                    middle.x - 19.0,
                    middle.y - 4.5,
                    middle.x + 19.0,
                    middle.y + 4.5
                ),
                red
            ) > 38 * 9 * 4 * 4 * 9 / 10
        );
        // Nothing of it outside the 40 x 10 it fits to.
        assert_eq!(
            colour_near(
                &with,
                &bx(
                    cell.min.x + 0.2,
                    middle.y - 4.0,
                    middle.x - 21.0,
                    middle.y + 4.0
                ),
                red
            ),
            0
        );
    }

    #[test]
    fn fields_fill_the_title_block_and_an_empty_field_prints_nothing() {
        let mut set = one_sheet();
        let model = Model::new();
        let source = source_of(&model);
        let cell = built_in_frame()
            .as_ref()
            .expect("the built-in frame reads")
            .cell("organisation")
            .expect("the frame has an organisation slot")
            .rect;
        let (empty, _) = painted(&set, &source, "fields_empty");
        assert_eq!(ink_in(&empty, &cell.inflated(-0.8)), 0);

        set.defaults.organisation = String::from("NORTHERN SURVEY CONSULTANTS");
        let (named, _) = painted(&set, &source, "fields_named");
        assert!(ink_in(&named, &cell.inflated(-0.8)) > 100);
    }

    #[test]
    fn the_legend_block_can_be_turned_off() {
        let mut set = one_sheet();
        let model = Model::new();
        let source = source_of(&model);
        let hydrant = &built_in_frame()
            .as_ref()
            .expect("the built-in frame reads")
            .symbols[0];
        let around = bx(
            hydrant.at.x - 0.4,
            hydrant.at.y - 0.4,
            hydrant.at.x + 0.4,
            hydrant.at.y + 0.4,
        );
        let (on, _) = painted(&set, &source, "the_legend_block_on");
        assert!(ink_in(&on, &around.inflated(0.4)) > 0);
        set.sheets[0].frame_legend = false;
        let (off, _) = painted(&set, &source, "the_legend_block_off");
        assert_eq!(ink_in(&off, &around.inflated(0.4)), 0);
    }

    #[test]
    fn a_plan_is_drawn_at_its_scale_and_clipped_to_its_viewport() {
        // A 100 m line at 1 : 1000 is 100 mm on the paper; one running far past the viewport
        // stops at its edge.
        let mut model = Model::new();
        add_line(&mut model, Point2::new(0.0, 0.0), Point2::new(100.0, 0.0));
        add_line(
            &mut model,
            Point2::new(-1000.0, 30.0),
            Point2::new(1000.0, 30.0),
        );
        let mut set = one_sheet();
        set.sheets[0].viewports.push(plan_at(
            bx(100.0, 100.0, 300.0, 250.0),
            1000.0,
            Point2::new(50.0, 0.0),
            0.0,
        ));
        let source = source_of(&model);
        let (paper, stats) = painted(&set, &source, "a_plan_is_drawn_at_its_scale");
        assert_eq!(stats.viewports_drawn, 1);
        assert!(stats.problems.is_empty());

        // The short line: from x 150 to 250 on the paper at y 175.
        assert!(ink_in(&paper, &bx(151.0, 174.5, 249.0, 175.5)) > 90 * 4 / 2);
        assert_eq!(ink_in(&paper, &bx(252.0, 173.0, 290.0, 174.0)), 0);
        // The long line at y 205: inside the viewport, and not outside it.
        assert!(ink_in(&paper, &bx(110.0, 204.5, 290.0, 205.5)) > 0);
        assert_eq!(ink_in(&paper, &bx(40.0, 203.0, 98.0, 207.0)), 0);
        assert_eq!(ink_in(&paper, &bx(302.0, 203.0, 400.0, 207.0)), 0);
    }

    #[test]
    fn a_rotated_viewport_runs_its_direction_across_the_paper() {
        // A line at 45 degrees, in a viewport turned to 45 degrees, runs level across the paper.
        let mut model = Model::new();
        add_line(
            &mut model,
            Point2::new(-50.0, -50.0),
            Point2::new(50.0, 50.0),
        );
        let mut set = one_sheet();
        set.sheets[0].viewports.push(plan_at(
            bx(100.0, 100.0, 300.0, 250.0),
            1000.0,
            Point2::new(0.0, 0.0),
            PI / 4.0,
        ));
        let source = source_of(&model);
        let (paper, _) = painted(&set, &source, "a_rotated_viewport");
        // Centre (200, 175); the 141 m line is 141 mm long, level.
        assert!(ink_in(&paper, &bx(140.0, 174.5, 260.0, 175.5)) > 100);
        assert_eq!(ink_in(&paper, &bx(190.0, 185.0, 210.0, 240.0)), 0);
    }

    #[test]
    fn an_aerial_photo_under_a_plan_viewport_prints_inside_it_and_nowhere_else() {
        // A 500 m square photo of 5 m pixels about the origin, all one blue, under a viewport
        // (100, 100)-(300, 250) at 1 : 1000 about the origin: the viewport shows 200 x 150 m of it
        // and is filled with it; nothing of it is printed beside, above or below the viewport,
        // though the photo runs 150 m past each side. Hidden in the Reference Data panel, it is
        // not printed at all.
        let blue = Color::rgb(40, 110, 200);
        let mut reference = ReferenceData::new();
        let id = reference.add(raster_of(
            100,
            100,
            [-250.0, 5.0, 0.0, 250.0, 0.0, -5.0],
            |_, _| blue,
        ));
        let model = Model::new();
        let mut set = one_sheet();
        set.sheets[0].viewports.push(plan_at(
            bx(100.0, 100.0, 300.0, 250.0),
            1000.0,
            Point2::new(0.0, 0.0),
            0.0,
        ));
        {
            let mut source = source_of(&model);
            source.plan.reference = Some(&reference);
            let (paper, _) = painted(&set, &source, "an_aerial_photo");
            let inside = 196 * 4 * 146 * 4;
            assert!(
                colour_near(&paper, &bx(102.0, 102.0, 298.0, 248.0), blue) > inside * 95 / 100
            );
            assert_eq!(
                colour_near(&paper, &bx(40.0, 100.0, 99.0, 250.0), blue),
                0
            );
            assert_eq!(
                colour_near(&paper, &bx(301.0, 100.0, 380.0, 250.0), blue),
                0
            );
            assert_eq!(
                colour_near(&paper, &bx(100.0, 251.0, 300.0, 280.0), blue),
                0
            );
            assert_eq!(
                colour_near(&paper, &bx(100.0, 40.0, 300.0, 99.0), blue),
                0
            );
        }

        reference
            .find_raster_mut(id)
            .expect("the photo is there")
            .visible = false;
        let mut source = source_of(&model);
        source.plan.reference = Some(&reference);
        let (hidden, _) = painted(&set, &source, "an_aerial_photo_hidden");
        assert_eq!(
            colour_near(&hidden, &bx(102.0, 102.0, 298.0, 248.0), blue),
            0
        );
    }

    #[test]
    fn a_turned_photo_in_a_turned_viewport_is_printed_where_its_geotransform_puts_it() {
        // An 80 x 80 image of 0.5 m pixels, transparent but for a red block of pixels 32..47 each
        // way, turned 30 degrees about its corner at (100, 50): a step along a row moves 0.5 (cos
        // 30, sin 30), a step down a column 0.5 (sin 30, -cos 30). The block's middle, pixel
        // corner (40, 40), is (100, 50) + 20 (1.366, -0.366) = (127.32, 42.68), and the block is
        // 8 m square. The viewport (100, 100)-(300, 250) at 1 : 1000 about (110, 40) is turned
        // 0.6 rad, so a model offset d from its centre lands at the paper's (200, 175) + R(-0.6) d
        // mm: d = (17.32, 2.68) is
        //   x = 17.32 cos 0.6 + 2.68 sin 0.6 = 14.295 + 1.513 = 15.808
        //   y = -17.32 sin 0.6 + 2.68 cos 0.6 = -9.780 + 2.212 = -7.568
        // so the block prints about (215.8, 167.4), 8 mm square turned, and not about (217.3,
        // 177.7), where an unturned viewport would put it. The transparent pixels print nothing:
        // all the red there is is the block's 64 mm^2, 1 024 pixels at 4 px/mm, less what the
        // smoothing at its edge - a ramp one image pixel, half a millimetre, wide - fades past
        // red.
        let turn = PI / 6.0;
        let c = turn.cos();
        let s = turn.sin();
        let mut reference = ReferenceData::new();
        reference.add(raster_of(
            80,
            80,
            [100.0, 0.5 * c, 0.5 * s, 50.0, 0.5 * s, -0.5 * c],
            |x, y| {
                if (32..48).contains(&x) && (32..48).contains(&y) {
                    Color::rgba(220, 0, 0, 255)
                } else {
                    Color::rgba(0, 0, 0, 0)
                }
            },
        ));
        let model = Model::new();
        let mut set = one_sheet();
        set.sheets[0].viewports.push(plan_at(
            bx(100.0, 100.0, 300.0, 250.0),
            1000.0,
            Point2::new(110.0, 40.0),
            0.6,
        ));
        let mut source = source_of(&model);
        source.plan.reference = Some(&reference);
        let (paper, _) = painted(&set, &source, "a_turned_photo");
        let red = Color::rgb(220, 0, 0);
        let d = Point2::new(
            100.0 + 20.0 * (c + s) - 110.0,
            50.0 + 20.0 * (s - c) - 40.0,
        );
        let expected = Point2::new(
            200.0 + d.x * 0.6_f64.cos() + d.y * 0.6_f64.sin(),
            175.0 - d.x * 0.6_f64.sin() + d.y * 0.6_f64.cos(),
        );
        assert!(near(expected.x, 215.808, 2e-3));
        assert!(near(expected.y, 167.432, 2e-3));
        // The middle 4 mm of the block - 16 or 17 pixels each way, as the box falls on the pixel
        // grid - is solid red.
        assert!(
            colour_near(
                &paper,
                &bx(
                    expected.x - 2.0,
                    expected.y - 2.0,
                    expected.x + 2.0,
                    expected.y + 2.0
                ),
                red
            ) >= 16 * 16
        );
        assert_eq!(
            colour_near(&paper, &bx(214.3, 174.7, 220.3, 180.7), red),
            0
        );
        let total = colour_near(&paper, &bx(100.0, 100.0, 300.0, 250.0), red);
        assert!(total > 1024 * 8 / 10, "{total}");
        assert!(total < 1024 * 12 / 10, "{total}");
    }

    #[test]
    fn a_sheet_pdf_over_a_large_photo_embeds_only_the_viewport_at_the_capped_resolution() {
        // A 2000 x 2000 photo of 2 cm pixels, 40 m square, of noise (which no compression
        // shrinks), filling a 40 mm viewport at 1 : 1000 on a sheet plotted at 600 dpi: a photo
        // pixel is 0.02 mm of paper, 0.47 of a device pixel. Not capped below the plot's
        // resolution it is averaged to the device's, 945 x 945 = 893 000 pixels; capped at 200 dpi
        // (the default) to 315 x 315 = 99 000, a ninth - and the PDF, which is almost all image,
        // shrinks with it. plot_sheets_to_pdf plots with the cap.
        let mut reference = ReferenceData::new();
        reference.add(raster_of(
            2000,
            2000,
            [-20.0, 0.02, 0.0, 20.0, 0.0, -0.02],
            |x, y| {
                let mut hash =
                    (x as u32).wrapping_mul(73_856_093) ^ (y as u32).wrapping_mul(19_349_663);
                hash ^= hash >> 13;
                hash = hash.wrapping_mul(0x5bd1_e995);
                hash ^= hash >> 15;
                Color::rgb(
                    (hash & 0xff) as u8,
                    ((hash >> 8) & 0xff) as u8,
                    ((hash >> 16) & 0xff) as u8,
                )
            },
        ));
        let model = Model::new();
        let mut set = one_sheet();
        set.sheets[0].viewports.push(plan_at(
            bx(100.0, 100.0, 140.0, 140.0),
            1000.0,
            Point2::new(0.0, 0.0),
            0.0,
        ));
        let mut source = source_of(&model);
        source.plan.reference = Some(&reference);
        let scratch = Scratch::new("sheet_painter_large_photo");
        let uncapped = sheet_pdf_bytes(&scratch.path("uncapped.pdf"), &set, &source, 600.0, 0.0);
        let capped = sheet_pdf_bytes(&scratch.path("capped.pdf"), &set, &source, 600.0, 200.0);
        assert!(
            capped * 4 < uncapped,
            "{capped} bytes capped, {uncapped} not"
        );

        let path = scratch.path("plotted.pdf");
        let mut cache = SheetPaintCache::new();
        plot_sheets_to_readable_pdf(&path, &set, &[], &source, 600.0, &mut cache, "", None)
            .expect("the sheet is plotted");
        let pdf = std::fs::read(&path).expect("the PDF is there");
        assert!(!places(&pdf, b"/Subtype /Image").is_empty());
        assert!(
            (pdf.len() as u64) * 4 < uncapped,
            "{} bytes plotted, {uncapped} not capped",
            pdf.len()
        );
        println!(
            "uncapped_bytes={uncapped} capped_bytes={capped} plotted_bytes={}",
            pdf.len()
        );
    }

    #[test]
    fn an_automatic_scale_is_reported_in_the_title_block() {
        let mut model = Model::new();
        add_line(&mut model, Point2::new(0.0, 0.0), Point2::new(300.0, 0.0));
        let mut viewport = plan_at(bx(23.0, 35.0, 410.0, 287.0), 1.0, Point2::default(), 0.0);
        viewport.auto_scale = true;
        viewport.auto_centre = true;
        let source = source_of(&model);
        // 300 m across 387 mm needs 1 : 806: the ladder's next is 1 : 1000.
        let resolved = resolve_plan_viewport(&viewport, &source);
        assert!(double_eq(resolved.scale, 1000.0), "{}", resolved.scale);
        assert!(near(resolved.centre.x, 150.0, 1e-9));
    }

    #[test]
    fn sections_are_cut_from_the_surfaces_along_the_alignment() {
        // Ground z = x / 2 under a road running north up x = 50.
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
        model
            .alignments
            .add(Alignment {
                name: String::from("ROAD"),
                horizontal: HorizontalAlignment {
                    pis: vec![
                        AlignmentPi {
                            point: Point2::new(50.0, 10.0),
                            ..AlignmentPi::default()
                        },
                        AlignmentPi {
                            point: Point2::new(50.0, 90.0),
                            ..AlignmentPi::default()
                        },
                    ],
                    ..HorizontalAlignment::default()
                },
                ..Alignment::default()
            })
            .expect("the road is added");

        let mut set = one_sheet();
        let mut profile = Viewport {
            id: String::from("vp1"),
            kind: ViewportKind::LongSection,
            rect: bx(23.0, 160.0, 410.0, 287.0),
            scale: 500.0,
            vertical_exaggeration: 10.0,
            auto_centre: true,
            ..Viewport::default()
        };
        profile.source.alignment = String::from("ROAD");
        let mut across = Viewport {
            id: String::from("vp2"),
            kind: ViewportKind::CrossSections,
            rect: bx(23.0, 35.0, 210.0, 155.0),
            scale: 250.0,
            auto_centre: true,
            ..Viewport::default()
        };
        across.source.alignment = String::from("ROAD");
        across.source.stations = vec![40.0];
        across.source.section_half_width = 20.0;
        set.sheets[0].viewports = vec![profile, across];

        let mut source = source_of(&model);
        source.surfaces.push(SceneSurface {
            name: String::from("GROUND"),
            surface: Some(&ground),
            ..SceneSurface::default()
        });
        let (paper, stats) = painted(&set, &source, "sections_are_cut");
        assert!(stats.problems.is_empty(), "{:?}", stats.problems);
        assert_eq!(stats.viewports_drawn, 2);
        // The cross section's ground: brown ink in its plot.
        assert!(
            colour_near(
                &paper,
                &bx(60.0, 60.0, 200.0, 150.0),
                Color::rgb(125, 80, 40)
            ) > 50
        );

        // Without a surface the cross section says why and draws nothing.
        source.surfaces.clear();
        let (_, stats) = painted(&set, &source, "sections_are_cut_without_surface");
        assert!(!stats.problems.is_empty());
        assert!(
            stats
                .problems
                .last()
                .is_some_and(|problem| problem.contains("vp2"))
        );
    }

    #[test]
    fn a_missing_alignment_is_reported_not_drawn() {
        let model = Model::new();
        let mut set = one_sheet();
        let mut profile = Viewport {
            id: String::from("vp7"),
            kind: ViewportKind::LongSection,
            rect: bx(23.0, 35.0, 410.0, 287.0),
            ..Viewport::default()
        };
        profile.source.alignment = String::from("NOWHERE");
        set.sheets[0].viewports.push(profile);
        let source = source_of(&model);
        let (_, stats) = painted(&set, &source, "a_missing_alignment");
        assert_eq!(stats.problems.len(), 1);
        assert_eq!(stats.problems[0], "vp7: no alignment named NOWHERE");
        assert_eq!(stats.viewports_drawn, 0);
    }

    #[test]
    fn notes_and_a_legend_draw_their_text() {
        let mut model = Model::new();
        add_line(&mut model, Point2::new(0.0, 0.0), Point2::new(10.0, 0.0));
        let mut set = one_sheet();
        let notes = Viewport {
            id: String::from("vp1"),
            kind: ViewportKind::Notes,
            rect: bx(340.0, 150.0, 400.0, 280.0),
            text: String::from(
                "ALL LEVELS ARE TO THE LOCAL DATUM. SERVICES SHOWN ARE APPROXIMATE AND MUST BE \
                 LOCATED BEFORE ANY EXCAVATION.",
            ),
            ..Viewport::default()
        };
        let legend = Viewport {
            id: String::from("vp2"),
            kind: ViewportKind::Legend,
            rect: bx(300.0, 40.0, 400.0, 145.0),
            ..Viewport::default()
        };
        set.sheets[0].viewports = vec![notes, legend];
        let source = source_of(&model);
        let (paper, _) = painted(&set, &source, "notes_and_a_legend");
        // Wrapped: text on at least three lines under the heading.
        let mut lines = 0;
        let mut y = 268.0;
        while y > 250.0 {
            if ink_in(&paper, &bx(343.0, y - 2.0, 395.0, y)) > 0 {
                lines += 1;
            }
            y -= 3.2;
        }
        assert!(lines >= 3, "{lines}");
        // The legend's one layer: a sample line and its name.
        assert!(ink_in(&paper, &bx(302.0, 128.0, 330.0, 136.0)) > 20);
    }

    #[test]
    fn the_whole_set_is_one_vector_pdf_a_page_a_sheet() {
        let mut model = Model::new();
        add_line(&mut model, Point2::new(0.0, 0.0), Point2::new(100.0, 50.0));
        let mut set = SheetSet::default();
        set.defaults.organisation = String::from("NORTHERN SURVEY CONSULTANTS");
        let mut one = a3("ONE");
        one.viewports.push(plan_at(
            bx(23.0, 35.0, 410.0, 287.0),
            500.0,
            Point2::new(50.0, 25.0),
            0.0,
        ));
        set.sheets.push(one);
        set.sheets.push(Sheet {
            id: String::from("s2"),
            paper: PaperSize::A1,
            ..a3("TWO")
        });
        set.sheets.push(Sheet {
            id: String::from("s3"),
            landscape: false,
            ..a3("THREE")
        });

        let scratch = Scratch::new("sheet_painter_whole_set");
        let path = scratch.path("set.pdf");
        let source = source_of(&model);
        let mut cache = SheetPaintCache::new();
        let mut problems = Vec::new();
        let status = plot_sheets_to_readable_pdf(
            &path,
            &set,
            &[],
            &source,
            300.0,
            &mut cache,
            "",
            Some(&mut problems),
        );
        assert!(status.is_ok(), "{:?}", status.err().map(|e| e.describe()));
        assert!(problems.is_empty(), "{problems:?}");
        let pdf = std::fs::read(&path).expect("the PDF is there");
        assert!(pdf.starts_with(b"%PDF"));
        // One /Page object per sheet (not counting the /Pages tree).
        let pages = places(&pdf, b"/Type /Page")
            .into_iter()
            .filter(|at| pdf.get(at + 11) != Some(&b's'))
            .count();
        assert_eq!(pages, 3);
        // The A1 page is 841 x 594 mm: 2384 x 1684 points.
        assert!(!places(&pdf, b"2383.9").is_empty() || !places(&pdf, b"2384").is_empty());

        // Only the sheets asked for, in the order asked.
        assert!(
            plot_sheets_to_pdf(&path, &set, &[1], &source, 300.0, &mut cache, "", None).is_ok()
        );
        assert!(
            plot_sheets_to_pdf(&path, &set, &[3], &source, 300.0, &mut cache, "", None).is_err()
        );
    }

    #[test]
    fn a_plan_drawn_without_a_surfaces_contours_says_so_in_the_sheets_problems_and_stays_drawn() {
        let model = Model::new();
        let store = ground_in("Fine", true);
        let styles = styles_with_fine();
        let mut source = source_of(&model);
        source.plan.surfaces = Some(&store);
        source.plan.surface_styles = Some(&styles);
        let (_, stats) = painted(
            &plan_over(Point2::new(5.0, 5.0)),
            &source,
            "a_plan_drawn_without_contours",
        );
        assert_eq!(stats.problems.len(), 1);
        // The viewport, the surface, its style and why - the words of the plan view's own log.
        assert_eq!(
            stats.problems[0],
            "vp1: ground (style Fine): contours: an interval of 0.000001 crosses this surface's \
             triangles more than 4000000 times; not drawn - set a larger interval"
        );
        // Not a viewport that could not be drawn: the plan is drawn, without the contours.
        assert_eq!(stats.viewports_drawn, 1);
    }

    #[test]
    fn a_surface_the_plan_can_draw_in_whole_or_need_not_draw_says_nothing() {
        let model = Model::new();
        let styles = styles_with_fine();
        let problems_of = |store: Option<&SurfaceStore>, centre: Point2| {
            let mut source = source_of(&model);
            source.plan.surface_styles = Some(&styles);
            source.plan.surfaces = store;
            painted(&plan_over(centre), &source, "a_surface_says_nothing")
                .1
                .problems
        };

        // The built-in Contours style: an interval of a unit, ten crossings a triangle.
        let traceable = ground_in("Contours", true);
        let problems = problems_of(Some(&traceable), Point2::new(5.0, 5.0));
        assert!(problems.is_empty(), "{problems:?}");

        // A style that shows no contours at all.
        let outline = ground_in("Standard", true);
        assert!(problems_of(Some(&outline), Point2::new(5.0, 5.0)).is_empty());

        // A hidden surface is not drawn, so it has no contours to be without.
        let hidden = ground_in("Fine", false);
        assert!(problems_of(Some(&hidden), Point2::new(5.0, 5.0)).is_empty());

        // A surface the plan's window does not reach is not drawn either.
        let refused = ground_in("Fine", true);
        assert!(problems_of(Some(&refused), Point2::new(5000.0, 5000.0)).is_empty());

        // And a drawing with no surfaces at all.
        assert!(problems_of(None, Point2::new(5.0, 5.0)).is_empty());
    }
}
