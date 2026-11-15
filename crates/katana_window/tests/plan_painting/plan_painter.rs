//! The twin of tests/qt_widgets/test_plan_painter.cpp's PlanPainter suite (the plan painter: what
//! it draws through a rotated frame, on paper and on screen, and what its screen-speed measures
//! leave unchanged), painted into canvas Images and read back pixel by pixel. Its three PlanView
//! cases are VP's (the plan view's kept drawing).

use std::collections::BTreeSet;
use std::sync::Arc;

use katana_cad::layer_overrides::LayerOverrides;
use katana_cad::plot::{PaperSize, PlotSettings};
use katana_cad::scene::SceneMesh;
use katana_cad::selection::SelectionSet;
use katana_cad::terrain::surface_plan::SurfaceProblem;
use katana_cad::terrain::surface_style::{SurfaceDisplayStyle, built_in_surface_styles};
use katana_cad::view_transform::ViewTransform;
use katana_entity::{
    Alignment, Color as EntityColor, DimensionGeometry, Entity, Geometry, HatchLineFamily,
    HatchPattern, LabelGeometry, Layer, Linetype, LinetypeElement, Model, PointGeometry, Style,
    TextGeometry,
};
use katana_geometry::{AlignmentPi, Circle2, Point2, Point3, Polyline2, Segment2, TriangleMesh};
use katana_interop::reference_data::{
    PointCloudLayer, PointColorMode, RasterOverlay, ReferenceData,
};
use katana_math::numerics::PI;
use katana_pointcloud::point_cloud_engine::PointCloudPoint;
use katana_terrain::surface_store::{NamedSurface, SurfaceStore};
use katana_terrain::tin_surface::TinSurface;
use katana_window::canvas::{Color, Image};
use katana_window::plan_painter::{
    PlanFrame, PlanMedium, PlanPaintCache, PlanPaintOptions, PlanPaintStats, PlanSource,
    paint_plan, pdf_resolution_for, plot_plan_to_pdf, sheet_frame, visible_box,
};

use crate::raster::{backend, paint_on, pdf_writer, written};
use crate::window_support::Scratch;

pub const BLACK: Color = Color::BLACK;
pub const WHITE: Color = Color::WHITE;

pub fn entity_of(geometry: Geometry, layer: &str) -> Entity {
    Entity {
        geometry,
        layer: String::from(layer),
        ..Entity::default()
    }
}

pub fn add(model: &mut Model, entity: Entity) -> u64 {
    model.entities.add(entity).expect("the entity is added")
}

/// A frame `width` x `height` pixels at `scale` pixels a unit about `centre`.
pub fn frame_of(
    width: f64,
    height: f64,
    scale: f64,
    centre: Point2,
    rotation: f64,
) -> PlanFrame<'static> {
    let mut transform = ViewTransform::default();
    transform.resize(width, height);
    transform.scale = scale;
    transform.center = centre;
    PlanFrame {
        transform,
        rotation,
        ..PlanFrame::default()
    }
}

/// The screen's options, with the raster backend a sprite is rasterised by and text measured
/// with (the window's).
pub fn screen() -> PlanPaintOptions<'static> {
    PlanPaintOptions {
        raster: Some(backend()),
        ..PlanPaintOptions::default()
    }
}

pub fn paper_options(settings: &PlotSettings, pixels_per_millimetre: f64) -> PlanPaintOptions<'_> {
    PlanPaintOptions {
        medium: PlanMedium::Paper,
        pixels_per_millimetre,
        plot: Some(settings),
        raster: Some(backend()),
        ..PlanPaintOptions::default()
    }
}

/// Paints `source` through `frame` onto a `fill`ed image of the frame's size, with `cache`.
pub fn painted_source_with(
    source: &PlanSource,
    frame: &PlanFrame,
    options: &PlanPaintOptions,
    fill: Color,
    cache: &mut PlanPaintCache,
) -> (Image, PlanPaintStats) {
    let mut image = Image::new(
        frame.transform.width_pixels as u32,
        frame.transform.height_pixels as u32,
    );
    image.fill(fill);
    let mut stats = PlanPaintStats::default();
    paint_on(&mut image, |canvas| {
        stats = paint_plan(canvas, source, frame, options, cache);
    });
    (image, stats)
}

pub fn painted_source(
    source: &PlanSource,
    frame: &PlanFrame,
    options: &PlanPaintOptions,
    fill: Color,
) -> (Image, PlanPaintStats) {
    painted_source_with(source, frame, options, fill, &mut PlanPaintCache::default())
}

/// Paints `model` through `frame` onto a `fill`ed image of the frame's size.
pub fn painted(
    model: &Model,
    frame: &PlanFrame,
    options: &PlanPaintOptions,
    fill: Color,
) -> (Image, PlanPaintStats) {
    let source = PlanSource {
        model: Some(model),
        ..PlanSource::default()
    };
    painted_source(&source, frame, options, fill)
}

pub fn painted_with(
    model: &Model,
    frame: &PlanFrame,
    options: &PlanPaintOptions,
    fill: Color,
    cache: &mut PlanPaintCache,
) -> (Image, PlanPaintStats) {
    let source = PlanSource {
        model: Some(model),
        ..PlanSource::default()
    };
    painted_source_with(&source, frame, options, fill, cache)
}

/// A `width` x `height` raster whose pixel (x, y) is `colour(x, y)`, placed by `geotransform`.
pub fn raster_of(
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

/// The geotransform of pixels `size` model units square, the image turned `angle`
/// counter-clockwise about its top-left corner at `origin`: a step along a row moves size (cos,
/// sin), a step down a column size (sin, -cos).
fn turned_geotransform(origin: Point2, size: f64, angle: f64) -> [f64; 6] {
    let c = angle.cos();
    let s = angle.sin();
    [origin.x, size * c, size * s, origin.y, size * s, -size * c]
}

/// qGray: (11 r + 16 g + 5 b) / 32.
pub fn grey(c: Color) -> i32 {
    (i32::from(c.r) * 11 + i32::from(c.g) * 16 + i32::from(c.b) * 5) / 32
}

/// Where the red channel of row `y` first falls through half, going right, in pixel-centre units
/// interpolated between the two pixels either side; NaN when it does not.
fn red_fall_along_row(image: &Image, y: u32) -> f64 {
    for x in 0..image.width() - 1 {
        let a = f64::from(image.pixel(x, y).r);
        let b = f64::from(image.pixel(x + 1, y).r);
        if a >= 127.5 && b < 127.5 {
            return f64::from(x) + (a - 127.5) / (a - b);
        }
    }
    f64::NAN
}

/// The same for the green channel down column `x`.
fn green_fall_down_column(image: &Image, x: u32) -> f64 {
    for y in 0..image.height() - 1 {
        let a = f64::from(image.pixel(x, y).g);
        let b = f64::from(image.pixel(x, y + 1).g);
        if a >= 127.5 && b < 127.5 {
            return f64::from(y) + (a - 127.5) / (a - b);
        }
    }
    f64::NAN
}

/// Whether `a` and `b` are within `tolerance` on every channel.
fn near(a: Color, b: Color, tolerance: i32) -> bool {
    (i32::from(a.r) - i32::from(b.r)).abs() <= tolerance
        && (i32::from(a.g) - i32::from(b.g)).abs() <= tolerance
        && (i32::from(a.b) - i32::from(b.b)).abs() <= tolerance
}

/// Whether any pixel within `radius` of (x, y) is not `background`.
fn ink_near(image: &Image, x: i32, y: i32, background: Color, radius: i32) -> bool {
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            let (px, py) = (x + dx, y + dy);
            if px >= 0
                && py >= 0
                && px < image.width() as i32
                && py < image.height() as i32
                && image.pixel(px as u32, py as u32) != background
            {
                return true;
            }
        }
    }
    false
}

/// How many pixels of row `y` are not `background`.
fn ink_in_row(image: &Image, y: u32, background: Color) -> i32 {
    (0..image.width())
        .filter(|&x| image.pixel(x, y) != background)
        .count() as i32
}

/// The whole image's darkness on white paper: the sum of 255 - grey.
fn ink(image: &Image) -> i64 {
    let mut total = 0;
    for y in 0..image.height() {
        for x in 0..image.width() {
            total += i64::from(255 - grey(image.pixel(x, y)));
        }
    }
    total
}

/// How many separate runs of ink lie along rows `y - 1` to `y + 1`, read column by column: a
/// column is ink when any of the three rows is.
fn runs_along(image: &Image, y: u32, background: Color) -> i32 {
    let mut runs = 0;
    let mut in_run = false;
    for x in 0..image.width() {
        let inked = (y - 1..=y + 1).any(|row| image.pixel(x, row) != background);
        runs += i32::from(inked && !in_run);
        in_run = inked;
    }
    runs
}

/// How many rows from `y` downwards in column `x` are ink without a break.
fn ink_rows_down_from(image: &Image, x: u32, y: u32, background: Color) -> i32 {
    let mut rows = 0;
    while y + rows < image.height() && image.pixel(x, y + rows) != background {
        rows += 1;
    }
    rows as i32
}

/// EXPECT_DOUBLE_EQ: within four units in the last place.
fn double_eq(a: f64, b: f64) -> bool {
    if a == b {
        return true;
    }
    let (x, y) = (a.to_bits() as i64, b.to_bits() as i64);
    (a.is_sign_negative() == b.is_sign_negative()) && (x - y).abs() <= 4
}

fn assert_near(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "{actual} is not within {tolerance} of {expected}"
    );
}

/// A white line from (10, 50) to (90, 50) on "design", selected, through a frame 100 x 100 px at
/// 1 px a unit about (50, 50): along row 50 from column 10 to 90, the frame's y running down. The
/// view hides "design" of its own.
struct GhostPlan {
    pub model: Model,
    pub selection: SelectionSet,
    pub view: LayerOverrides,
    /// Whether the frame carries the view's hidden layers.
    pub hidden_in_view: bool,
    pub options: PlanPaintOptions<'static>,
}

impl GhostPlan {
    fn new() -> Self {
        let mut model = Model::new();
        model
            .layers
            .add(Layer {
                name: String::from("design"),
                color: EntityColor {
                    r: 255,
                    g: 255,
                    b: 255,
                    a: 255,
                },
                ..Layer::default()
            })
            .expect("the layer is added");
        let id = add(
            &mut model,
            entity_of(
                Geometry::Line(Segment2::new(
                    Point2::new(10.0, 50.0),
                    Point2::new(90.0, 50.0),
                )),
                "design",
            ),
        );
        let mut selection = SelectionSet::default();
        selection.add(id);
        let mut view = LayerOverrides::default();
        assert!(view.hide("design"));
        Self {
            model,
            selection,
            view,
            hidden_in_view: true,
            options: PlanPaintOptions {
                selection_ghosts: true,
                ..screen()
            },
        }
    }

    pub fn frame(&self) -> PlanFrame<'_> {
        PlanFrame {
            layers: if self.hidden_in_view {
                Some(&self.view)
            } else {
                None
            },
            ..frame_of(100.0, 100.0, 1.0, Point2::new(50.0, 50.0), 0.0)
        }
    }

    pub fn paint(&self, selected: bool) -> (Image, PlanPaintStats) {
        let none = SelectionSet::default();
        let source = PlanSource {
            model: Some(&self.model),
            selection: Some(if selected { &self.selection } else { &none }),
            ..PlanSource::default()
        };
        painted_source(&source, &self.frame(), &self.options, BLACK)
    }
}

/// Pixels of the line's rows, 48 to 52, in the selection's orange: red well above green, green
/// well above blue - never the layer's white, the black ground or grey.
fn orange_ink(image: &Image) -> usize {
    let mut count = 0;
    for y in 48..=52 {
        for x in 0..image.width() {
            let c = image.pixel(x, y);
            let (r, g, b) = (i32::from(c.r), i32::from(c.g), i32::from(c.b));
            count += usize::from(r > g + 30 && g > b + 30);
        }
    }
    count
}

/// The pixels the ghost inked: those that differ from the frame painted with nothing selected.
fn ghost_ink(plan: &GhostPlan) -> (Vec<(i32, i32)>, PlanPaintStats) {
    let (bare, _) = plan.paint(false);
    let (ghosted, stats) = plan.paint(true);
    let mut ink = Vec::new();
    for y in 0..ghosted.height() {
        for x in 0..ghosted.width() {
            if ghosted.pixel(x, y) != bare.pixel(x, y) {
                ink.push((x as i32, y as i32));
            }
        }
    }
    (ink, stats)
}

/// How many separate marks the ink makes: pixels grouped with their eight neighbours. A dotted
/// trace is many; a solid one is one.
fn marks_in(ink: &[(i32, i32)]) -> i32 {
    let mut seen = vec![false; ink.len()];
    let mut marks = 0;
    for first in 0..ink.len() {
        if seen[first] {
            continue;
        }
        marks += 1;
        let mut open = vec![first];
        seen[first] = true;
        while let Some(at) = open.pop() {
            let (x, y) = ink[at];
            for other in 0..ink.len() {
                if !seen[other] && (ink[other].0 - x).abs() <= 1 && (ink[other].1 - y).abs() <= 1 {
                    seen[other] = true;
                    open.push(other);
                }
            }
        }
    }
    marks
}

/// Selects `geometry`, alone, on GhostPlan's hidden layer.
fn select_only(plan: &mut GhostPlan, geometry: Geometry) {
    let id = add(&mut plan.model, entity_of(geometry, "design"));
    plan.selection.set(vec![id]);
}

/// The ink is the dotted outline of a box: every pixel within a pixel of an edge of the box round
/// the ink, none inside it, and separate dots along each of its four sides. Returns that box, the
/// ink's own bounds, as (left, top, right, bottom).
fn expect_dotted_box(ink: &[(i32, i32)]) -> (i32, i32, i32, i32) {
    assert!(!ink.is_empty());
    let left = ink.iter().map(|p| p.0).min().unwrap_or(0);
    let right = ink.iter().map(|p| p.0).max().unwrap_or(0);
    let top = ink.iter().map(|p| p.1).min().unwrap_or(0);
    let bottom = ink.iter().map(|p| p.1).max().unwrap_or(0);
    let mut sides: [Vec<(i32, i32)>; 4] = Default::default(); // top, bottom, left, right
    for &(x, y) in ink {
        let on_top = y <= top + 1;
        let on_bottom = y >= bottom - 1;
        let on_left = x <= left + 1;
        let on_right = x >= right - 1;
        assert!(
            on_top || on_bottom || on_left || on_right,
            "ink inside the box at {x},{y}"
        );
        if on_top {
            sides[0].push((x, y));
        }
        if on_bottom {
            sides[1].push((x, y));
        }
        if on_left {
            sides[2].push((x, y));
        }
        if on_right {
            sides[3].push((x, y));
        }
    }
    for side in &sides {
        assert!(marks_in(side) >= 2, "dots along each side, not one mark");
    }
    (left, top, right, bottom)
}

fn sorted(mut points: Vec<(i32, i32)>) -> Vec<(i32, i32)> {
    points.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)));
    points
}

/// The 10 x 10 square z = x, two triangles.
fn square_tin() -> Arc<TinSurface> {
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

mod plan_painter {
    use super::*;

    // ---- rotated frames ----

    #[test]
    fn a_rotated_frame_culls_by_the_box_around_the_turned_rectangle() {
        // A 200 x 100 frame at 1 px a unit about the origin, turned 30 degrees. The turned
        // rectangle's corners reach, about the centre,
        //   across: 100 cos 30 + 50 sin 30 = 86.603 + 25 = 111.603
        //   up:     100 sin 30 + 50 cos 30 = 50 + 43.301 = 93.301
        // so the box it shows is (-111.603, -93.301)-(111.603, 93.301), where the unturned frame
        // shows (-100, -50)-(100, 50).
        let turn = PI / 6.0;
        let bounds = visible_box(&frame_of(200.0, 100.0, 1.0, Point2::new(0.0, 0.0), turn));
        let across = 100.0 * turn.cos() + 50.0 * turn.sin();
        let up = 100.0 * turn.sin() + 50.0 * turn.cos();
        assert_near(across, 111.603, 1e-3);
        assert_near(up, 93.301, 1e-3);
        assert_near(bounds.min.x, -across, 1e-9);
        assert_near(bounds.max.x, across, 1e-9);
        assert_near(bounds.min.y, -up, 1e-9);
        assert_near(bounds.max.y, up, 1e-9);
    }

    #[test]
    fn an_entity_in_the_corner_of_a_turned_frame_is_drawn_there_and_not_culled() {
        // The same 200 x 100 frame turned a quarter turn counter-clockwise. The drawing's north
        // (model +y, up the screen) now points left, and its east points up: a model offset
        // (e, n) from the centre lands at the device offset (-n, -e), y down. The segment
        // (45, 95)-(48, 95) therefore lands at x = 100 - 95 = 5, from y = 50 - 45 = 5 up to
        // 50 - 48 = 2: in the top-left corner. Unturned, y = 95 is far above the frame's y <= 50
        // and the segment is culled.
        let mut model = Model::new();
        add(
            &mut model,
            entity_of(
                Geometry::Line(Segment2::new(
                    Point2::new(45.0, 95.0),
                    Point2::new(48.0, 95.0),
                )),
                "0",
            ),
        );
        let options = screen();
        let (image, turned) = painted(
            &model,
            &frame_of(200.0, 100.0, 1.0, Point2::new(0.0, 0.0), PI / 2.0),
            &options,
            BLACK,
        );
        assert_eq!(turned.entities_drawn, 1);
        assert!(ink_near(&image, 5, 3, BLACK, 1));
        // Nowhere near where the unturned frame would have put it (off the top).
        assert!(!ink_near(&image, 145, 3, BLACK, 3));

        let (_, straight) = painted(
            &model,
            &frame_of(200.0, 100.0, 1.0, Point2::new(0.0, 0.0), 0.0),
            &options,
            BLACK,
        );
        assert_eq!(straight.entities_drawn, 0);
    }

    #[test]
    fn an_entity_at_the_corner_of_a_frame_turned_thirty_degrees_is_drawn_at_that_corner() {
        // Device pixel (8, 8)'s centre, (8.5, 8.5), is the offset (-91.5, -41.5) from the centre
        // of the 200 x 100 frame, y down: (-91.5, 41.5) y up. The drawing is turned 30 degrees
        // counter-clockwise, so the model offset there is that turned back 30 degrees:
        //   e = cos30 (-91.5) + sin30 (41.5) = -79.241 + 20.750 = -58.491
        //   n = -sin30 (-91.5) + cos30 (41.5) = 45.750 + 35.940 = 81.690
        // n = 81.69 is above the unturned frame's y <= 50, so only the turned box keeps it. A
        // 2-unit segment centred there is drawn through (8, 8).
        let c = (PI / 6.0).cos();
        let s = (PI / 6.0).sin();
        let at = Point2::new(c * -91.5 + s * 41.5, -s * -91.5 + c * 41.5);
        assert_near(at.x, -58.491, 1e-3);
        assert_near(at.y, 81.690, 1e-3);
        let mut model = Model::new();
        add(
            &mut model,
            entity_of(
                Geometry::Line(Segment2::new(
                    Point2::new(at.x - 1.0, at.y),
                    Point2::new(at.x + 1.0, at.y),
                )),
                "0",
            ),
        );
        let (image, stats) = painted(
            &model,
            &frame_of(200.0, 100.0, 1.0, Point2::new(0.0, 0.0), PI / 6.0),
            &screen(),
            BLACK,
        );
        assert_eq!(stats.entities_drawn, 1);
        assert!(ink_near(&image, 8, 8, BLACK, 1));
    }

    // ---- paper ----

    #[test]
    fn a_plots_frame_is_the_printable_area_clipped_and_centred_on_the_paper() {
        // A4 landscape at 254 dpi is 10 device pixels a millimetre: the paper is 2970 x 2100, the
        // 10 mm margins 100 px, the printable area 2770 x 1900.
        let settings = PlotSettings {
            paper: PaperSize::A4,
            landscape: true,
            dpi: 254.0,
            margin_mm: 10.0,
            scale_denominator: 1000.0,
            center: Point2::new(500.0, 300.0),
            ..PlotSettings::default()
        };
        let frame = sheet_frame(&settings).expect("the sheet has a frame");
        assert!(frame.clip);
        assert_near(frame.origin.x, 100.0, 1e-9);
        assert_near(frame.origin.y, 100.0, 1e-9);
        assert_near(frame.transform.width_pixels, 2770.0, 1e-9);
        assert_near(frame.transform.height_pixels, 1900.0, 1e-9);
        // The sheet's centre point is the paper's centre, (1485, 1050).
        let centre = frame.transform.world_to_screen(settings.center);
        assert_near(centre.x + frame.origin.x, 1485.0, 1e-9);
        assert_near(centre.y + frame.origin.y, 1050.0, 1e-9);
    }

    #[test]
    fn at_a_fixed_scale_the_drawing_stops_at_the_margin_of_the_paper() {
        // A4 landscape at 101.6 dpi, 4 px a millimetre: 1188 x 840 px of paper, 40 px margins. At
        // 1 : 1000 a metre is a millimetre, 4 px. The line from x = -500 to 500 m through the
        // centre is 4000 px long, far wider than the paper: it must stop 40 px from each edge.
        let settings = PlotSettings {
            paper: PaperSize::A4,
            landscape: true,
            dpi: 101.6,
            margin_mm: 10.0,
            scale_denominator: 1000.0,
            center: Point2::new(0.0, 0.0),
            ..PlotSettings::default()
        };
        let mut model = Model::new();
        add(
            &mut model,
            entity_of(
                Geometry::Line(Segment2::new(
                    Point2::new(-500.0, 0.0),
                    Point2::new(500.0, 0.0),
                )),
                "0",
            ),
        );
        let frame = sheet_frame(&settings).expect("the sheet has a frame");
        let mut paper = Image::new(1188, 840);
        paper.fill(WHITE);
        let source = PlanSource {
            model: Some(&model),
            ..PlanSource::default()
        };
        let mut cache = PlanPaintCache::default();
        let options = paper_options(&settings, 4.0);
        paint_on(&mut paper, |canvas| {
            paint_plan(canvas, &source, &frame, &options, &mut cache);
        });
        assert!(ink_near(&paper, 594, 420, WHITE, 1)); // the middle of the sheet
        assert!(ink_near(&paper, 45, 420, WHITE, 1)); // just inside the margin
        assert!(!ink_near(&paper, 20, 420, WHITE, 2)); // in the left margin
        assert!(!ink_near(&paper, 1168, 420, WHITE, 2)); // in the right margin
    }

    #[test]
    fn a_point_cross_on_paper_is_two_millimetres_across_at_every_resolution() {
        // A plain point's cross is POINT_MARKER_PAPER_MILLIMETRES = 1 mm either side on paper:
        // 8 px across at 4 px/mm and 16 px at 8 px/mm (plus up to a pixel of antialiasing at each
        // end). It was 4 px either side whatever the resolution, 8 px at both.
        let settings = PlotSettings::default();
        let mut model = Model::new();
        add(
            &mut model,
            entity_of(
                Geometry::Point(PointGeometry {
                    position: Point2::new(0.0, 0.0),
                }),
                "0",
            ),
        );
        let cross_width = |pixels_per_millimetre: f64| {
            let (image, _) = painted(
                &model,
                &frame_of(100.0, 100.0, 1.0, Point2::new(0.0, 0.0), 0.0),
                &paper_options(&settings, pixels_per_millimetre),
                WHITE,
            );
            ink_in_row(&image, 50, WHITE)
        };
        let coarse = cross_width(4.0);
        let fine = cross_width(8.0);
        assert!((8..=10).contains(&coarse), "{coarse}");
        assert!((16..=18).contains(&fine), "{fine}");
    }

    #[test]
    fn a_solid_hatch_is_opaque_on_paper_and_translucent_on_screen() {
        // A 40 x 40 square on a layer hatched solid, in red. On paper its middle is the layer's
        // red itself; on screen, the red at alpha 90 over black: 255 * 90 / 255 = 90, so
        // (90, 0, 0) give or take the rounding.
        let mut model = Model::new();
        model
            .hatch_patterns
            .add(HatchPattern {
                name: String::from("fill"),
                solid: true,
                ..HatchPattern::default()
            })
            .expect("the pattern is added");
        model
            .layers
            .add(Layer {
                name: String::from("lots"),
                color: EntityColor {
                    r: 255,
                    g: 0,
                    b: 0,
                    a: 255,
                },
                hatch_pattern: String::from("fill"),
                ..Layer::default()
            })
            .expect("the layer is added");
        add(
            &mut model,
            entity_of(
                Geometry::Polyline(Polyline2::new(
                    vec![
                        Point2::new(-20.0, -20.0),
                        Point2::new(20.0, -20.0),
                        Point2::new(20.0, 20.0),
                        Point2::new(-20.0, 20.0),
                    ],
                    true,
                )),
                "lots",
            ),
        );
        let frame = frame_of(100.0, 100.0, 1.0, Point2::new(0.0, 0.0), 0.0);
        let settings = PlotSettings::default();
        let (paper, _) = painted(&model, &frame, &paper_options(&settings, 4.0), WHITE);
        assert_eq!(paper.pixel(50, 50), Color::rgb(255, 0, 0));
        let (screen_image, _) = painted(&model, &frame, &screen(), BLACK);
        assert!((i32::from(screen_image.pixel(50, 50).r) - 90).abs() <= 1);
        assert_eq!(screen_image.pixel(50, 50).g, 0);
    }

    #[test]
    fn hatch_lines_on_paper_are_thirteen_hundredths_of_a_millimetre_wide_at_every_resolution() {
        // An 80 x 80 square about the origin, hatched with horizontal lines 20 units apart, in
        // black on white paper at 1 px a unit. Down the middle column, rows 20 to 79 are exactly
        // three spacings, so they hold exactly three lines' worth of ink wherever the family's
        // offset puts them, and well clear of the boundary at rows 10 and 90 and its 0.25 mm pen.
        // The column's darkness over 255 is a line's width in pixels, three times:
        // HATCH_LINE_PAPER_MILLIMETRES = 0.13 mm is 0.13 * 20 = 2.6 px at 20 px/mm and 5.2 px at
        // 40 px/mm. A hairline is one device pixel at both.
        let mut model = Model::new();
        model
            .hatch_patterns
            .add(HatchPattern {
                name: String::from("rule"),
                families: vec![HatchLineFamily {
                    angle: 0.0,
                    spacing: 20.0,
                    offset: 5.0,
                }],
                ..HatchPattern::default()
            })
            .expect("the pattern is added");
        model
            .layers
            .add(Layer {
                name: String::from("ruled"),
                color: EntityColor {
                    r: 0,
                    g: 0,
                    b: 0,
                    a: 255,
                },
                hatch_pattern: String::from("rule"),
                ..Layer::default()
            })
            .expect("the layer is added");
        add(
            &mut model,
            entity_of(
                Geometry::Polyline(Polyline2::new(
                    vec![
                        Point2::new(-40.0, -40.0),
                        Point2::new(40.0, -40.0),
                        Point2::new(40.0, 40.0),
                        Point2::new(-40.0, 40.0),
                    ],
                    true,
                )),
                "ruled",
            ),
        );
        let settings = PlotSettings::default();
        let line_width = |pixels_per_millimetre: f64| {
            let (image, _) = painted(
                &model,
                &frame_of(100.0, 100.0, 1.0, Point2::new(0.0, 0.0), 0.0),
                &paper_options(&settings, pixels_per_millimetre),
                WHITE,
            );
            let mut darkness = 0.0;
            for y in 20..80 {
                darkness += f64::from(255 - grey(image.pixel(50, y))) / 255.0;
            }
            darkness / 3.0
        };
        assert_near(line_width(20.0), 2.6, 0.3);
        assert_near(line_width(40.0), 5.2, 0.3);
    }

    #[test]
    fn the_alignment_overlay_on_paper_is_millimetres_of_paper_at_every_resolution() {
        // A straight alignment from (-60, 0) to (60, 0) plotted twice: at 4 px/mm through a frame
        // of 1 px a unit, and at 8 px/mm through one of 2 px a unit - the same sheet at twice the
        // resolution. With k = 1 and 2 the centreline lies along row 50k and the key stations are
        // the ends, 0+000 at column 20k and 0+120 at 140k.
        //
        // The end tick is ALIGNMENT_TICK_PAPER_MILLIMETRES = 1.5 mm either side of the
        // centreline: 6 px below it at 4 px/mm and 12 px at 8 px/mm, plus half the 0.25 mm tick
        // pen's square cap (0.5 and 1 px) and a pixel of antialiasing - rows 50k down to 56 or 57
        // at k = 1, and to 112 or 113 at k = 2. Nothing else is below the line there: the
        // station's label is above it, and the alignment's name is at the start.
        //
        // Everything else scales the same way - the 0.5 mm centreline, the 2.5 mm labels and the
        // gap between them - so at twice the resolution the overlay is the same picture twice the
        // size: four times the ink, within an eighth for the antialiased edges and the glyphs'
        // rasterising at two sizes.
        let mut model = Model::new();
        model
            .alignments
            .add(Alignment {
                name: String::from("A"),
                horizontal: katana_geometry::HorizontalAlignment {
                    pis: vec![
                        AlignmentPi {
                            point: Point2::new(-60.0, 0.0),
                            ..AlignmentPi::default()
                        },
                        AlignmentPi {
                            point: Point2::new(60.0, 0.0),
                            ..AlignmentPi::default()
                        },
                    ],
                    ..katana_geometry::HorizontalAlignment::default()
                },
                ..Alignment::default()
            })
            .expect("the alignment is added");
        let settings = PlotSettings::default();
        let sheet = |k: f64| {
            painted(
                &model,
                &frame_of(160.0 * k, 100.0 * k, k, Point2::new(0.0, 0.0), 0.0),
                &paper_options(&settings, 4.0 * k),
                WHITE,
            )
            .0
        };
        let coarse = sheet(1.0);
        let fine = sheet(2.0);
        let coarse_tick = ink_rows_down_from(&coarse, 140, 50, WHITE);
        let fine_tick = ink_rows_down_from(&fine, 280, 100, WHITE);
        assert!((6..=8).contains(&coarse_tick), "{coarse_tick}");
        assert!((12..=14).contains(&fine_tick), "{fine_tick}");
        let ratio = ink(&fine) as f64 / ink(&coarse) as f64;
        assert!(ratio > 3.5 && ratio < 4.5, "{ratio}");
    }

    #[test]
    fn text_on_paper_is_set_at_its_fractional_height_from_one_cached_font() {
        // At 1 px a unit, a text 30.0 units tall is 30.0 px and one 30.9 tall is 30.9 px. A
        // whole-pixel font set both at 30 px, the same ink; set at its exact height the taller
        // one's ink grows with its area, by about (30.9 / 30)^2 = 1.061. Asked for more than 3%
        // so the test is not about the font's rounding. Every size is one font on paper.
        let settings = PlotSettings::default();
        let mut cache = PlanPaintCache::default();
        let mut ink_of = |height: f64| {
            let mut model = Model::new();
            add(
                &mut model,
                entity_of(
                    Geometry::Text(TextGeometry {
                        position: Point2::new(-90.0, -10.0),
                        text: String::from("HEIGHT"),
                        height,
                        rotation: 0.0,
                        ..TextGeometry::default()
                    }),
                    "0",
                ),
            );
            ink(&painted_with(
                &model,
                &frame_of(200.0, 100.0, 1.0, Point2::new(0.0, 0.0), 0.0),
                &paper_options(&settings, 4.0),
                WHITE,
                &mut cache,
            )
            .0)
        };
        let whole = ink_of(30.0);
        let fractional = ink_of(30.9);
        assert!(whole > 0);
        assert!(fractional as f64 > 1.03 * whole as f64);
        assert_eq!(cache.font_count(), 1);
    }

    #[test]
    fn a_pdf_is_given_the_nearest_whole_resolution_and_the_scale_to_the_exact_one() {
        // Audit QT-27. 300.9 dpi: the writer at 301, the canvas scaled by 301 / 300.9 so the
        // sheet's pixels of 1/300.9 inch land exactly.
        let odd = pdf_resolution_for(300.9);
        assert_eq!(odd.resolution, 301);
        assert!(double_eq(odd.scale, 301.0 / 300.9));
        let even = pdf_resolution_for(300.0);
        assert_eq!(even.resolution, 300);
        assert_eq!(even.scale.to_bits(), 1.0_f64.to_bits());
        // Never a resolution of nothing.
        assert_eq!(pdf_resolution_for(0.4).resolution, 1);
    }

    // ---- imagery on paper ----

    #[test]
    fn a_raster_on_paper_lands_where_the_screen_draws_the_whole_image() {
        // A 256 x 256 image of 0.25 m pixels, red in its left half and green in its top half: an
        // edge through its middle in each channel. Through a 240 x 160 frame at 6.4 px a metre an
        // image pixel is 1.6 device pixels. On paper at 8 px/mm with a cap of 63.5 dpi (2.5 px/mm,
        // 0.3125 of the device's) it is half a capped pixel, so the crop is averaged down by two;
        // with no cap it is embedded as it is. Either way the red edge must cross each row, and
        // the green edge each column, where the screen - which draws the whole image through the
        // one composed transform - puts it, to within a device pixel.
        //
        // Straight; the image turned 25 degrees by its geotransform; the frame turned 0.35 rad (20
        // degrees); and both, 25 degrees and 0.2 rad (36.5 degrees in all - under 45, so the red
        // edge still crosses every row tested and the green every column). The view's centre is
        // 1.5 m east and 1 m south of the image's middle, so neither edge runs through the frame's
        // centre, about which it turns.
        let settings = PlotSettings::default();
        let degrees25 = 25.0 * PI / 180.0;
        for (image_turn, frame_turn) in
            [(0.0, 0.0), (degrees25, 0.0), (0.0, 0.35), (degrees25, 0.2)]
        {
            let g = turned_geotransform(Point2::new(1000.0, 2000.0), 0.25, image_turn);
            let mut reference = ReferenceData::new();
            reference.add(raster_of(256, 256, g, |x, y| {
                Color::rgb(
                    if x < 128 { 255 } else { 0 },
                    if y < 128 { 255 } else { 0 },
                    60,
                )
            }));
            let middle = Point2::new(g[0] + 128.0 * (g[1] + g[2]), g[3] + 128.0 * (g[4] + g[5]));
            let frame = frame_of(
                240.0,
                160.0,
                6.4,
                Point2::new(middle.x + 1.5, middle.y - 1.0),
                frame_turn,
            );
            let source = PlanSource {
                reference: Some(&reference),
                ..PlanSource::default()
            };
            let (screen_image, screen_stats) = painted_source(&source, &frame, &screen(), BLACK);
            assert_eq!(screen_stats.rasters_drawn, 1);
            for cap in [63.5, 0.0] {
                let paper = PlanPaintOptions {
                    raster_dpi_cap: cap,
                    ..paper_options(&settings, 8.0)
                };
                let (printed, stats) = painted_source(&source, &frame, &paper, BLACK);
                assert_eq!(stats.rasters_drawn, 1);
                assert_eq!(stats.raster_crops_made, 1);
                let mut worst: f64 = 0.0;
                for y in (20..=140).step_by(5) {
                    let expected = red_fall_along_row(&screen_image, y);
                    let actual = red_fall_along_row(&printed, y);
                    assert!(!expected.is_nan(), "row {y}");
                    assert!(!actual.is_nan(), "row {y}");
                    worst = worst.max((expected - actual).abs());
                }
                for x in (60..=180).step_by(5) {
                    let expected = green_fall_down_column(&screen_image, x);
                    let actual = green_fall_down_column(&printed, x);
                    assert!(!expected.is_nan(), "column {x}");
                    assert!(!actual.is_nan(), "column {x}");
                    worst = worst.max((expected - actual).abs());
                }
                assert!(
                    worst <= 1.0,
                    "image turned {image_turn}, frame turned {frame_turn}, cap {cap}: {worst}"
                );
            }
        }
    }

    #[test]
    fn a_raster_on_paper_is_cropped_to_the_viewport_and_embedded_no_finer_than_the_cap() {
        // A 2000 x 2000 image of 5 cm pixels, 100 m square, under a 200 x 100 frame at 10 px a
        // metre about its middle - 1 : 1000 at 10 px/mm (254 dpi). The frame shows 20 x 10 m of
        // it, 400 x 200 of its pixels, each half a device pixel; with a pixel of margin all round
        // the crop is 402 x 202. The whole image at its own resolution would be 4 000 000 pixels.
        //   No dpi cap: averaged down to the device's own resolution, one pixel a device pixel:
        //     402 / 2 x 202 / 2 = 201 x 101, 20 301.
        //   100 dpi: 3.937 px/mm, 0.3937 of the device's, so a pixel is 0.19685 of a capped one:
        //     402 x 0.19685 = 79 by 202 x 0.19685 = 40, 3 160.
        //   100 dpi and a pixel cap of 1 000: at most 1 000.
        let mut reference = ReferenceData::new();
        reference.add(raster_of(
            2000,
            2000,
            [0.0, 0.05, 0.0, 100.0, 0.0, -0.05],
            |_, _| Color::rgb(30, 140, 90),
        ));
        let source = PlanSource {
            reference: Some(&reference),
            ..PlanSource::default()
        };
        let frame = frame_of(200.0, 100.0, 10.0, Point2::new(50.0, 50.0), 0.0);
        let settings = PlotSettings::default();
        let embedded = |dpi_cap: f64, pixel_cap: usize| {
            let paper = PlanPaintOptions {
                raster_dpi_cap: dpi_cap,
                raster_pixel_cap: pixel_cap,
                ..paper_options(&settings, 10.0)
            };
            let (printed, stats) = painted_source(&source, &frame, &paper, WHITE);
            assert_eq!(stats.rasters_drawn, 1);
            // The image's colour, all over the viewport, at every resolution.
            for (x, y) in [(2, 2), (100, 50), (197, 97)] {
                assert!(
                    near(printed.pixel(x, y), Color::rgb(30, 140, 90), 2),
                    "{x},{y} cap {dpi_cap}/{pixel_cap}"
                );
            }
            stats.raster_pixels_embedded as f64
        };
        assert_near(embedded(0.0, 16_000_000), 20_301.0, 400.0);
        assert_near(embedded(100.0, 16_000_000), 3_160.0, 150.0);
        let capped = embedded(100.0, 1_000);
        assert!(capped <= 1_000.0);
        assert!(capped > 900.0);
    }

    #[test]
    fn a_fine_raster_is_averaged_down_on_paper_and_not_point_sampled() {
        // A 400 x 400 one-pixel black and white checkerboard of 1 cm pixels, exactly filling a
        // 100 x 100 frame at 25 px a metre: four image pixels to a device pixel each way, and the
        // crop - its margin clamped to the image - the whole image. On paper with the dpi cap off
        // it is averaged down to the device's resolution, 100 x 100, four to one, and every box of
        // 4 x 4 checks is half white: 127.5, mid grey, everywhere, and drawn 1 : 1. Sampled at the
        // nearest pixel it would print a moire of black and white.
        let mut reference = ReferenceData::new();
        reference.add(raster_of(
            400,
            400,
            [0.0, 0.01, 0.0, 4.0, 0.0, -0.01],
            |x, y| {
                if (x + y) % 2 == 0 {
                    Color::rgb(255, 255, 255)
                } else {
                    Color::rgb(0, 0, 0)
                }
            },
        ));
        let source = PlanSource {
            reference: Some(&reference),
            ..PlanSource::default()
        };
        let settings = PlotSettings::default();
        let paper = PlanPaintOptions {
            raster_dpi_cap: 0.0,
            ..paper_options(&settings, 10.0)
        };
        let (printed, stats) = painted_source(
            &source,
            &frame_of(100.0, 100.0, 25.0, Point2::new(2.0, 2.0), 0.0),
            &paper,
            WHITE,
        );
        assert_eq!(stats.raster_pixels_embedded, 100 * 100);
        let mut far = 0;
        for y in 0..100 {
            for x in 0..100 {
                far += i32::from((grey(printed.pixel(x, y)) - 128).abs() > 2);
            }
        }
        assert_eq!(far, 0);
    }

    #[test]
    fn a_hidden_raster_or_one_outside_the_viewport_does_not_print() {
        let mut reference = ReferenceData::new();
        let id = reference.add(raster_of(
            100,
            100,
            [0.0, 1.0, 0.0, 100.0, 0.0, -1.0],
            |_, _| Color::rgb(220, 0, 0),
        ));
        let settings = PlotSettings::default();
        let paper = paper_options(&settings, 4.0);
        let mut frame = frame_of(100.0, 100.0, 2.0, Point2::new(50.0, 50.0), 0.0);
        {
            let source = PlanSource {
                reference: Some(&reference),
                ..PlanSource::default()
            };
            let (image, stats) = painted_source(&source, &frame, &paper, WHITE);
            assert!(near(image.pixel(50, 50), Color::rgb(220, 0, 0), 2));
            assert_eq!(stats.rasters_drawn, 1);

            // Hidden in this view only.
            let hidden: BTreeSet<u64> = BTreeSet::from([id]);
            let hiding = PlanFrame {
                hidden_references: Some(&hidden),
                ..frame
            };
            let (image, stats) = painted_source(&source, &hiding, &paper, WHITE);
            assert_eq!(image.pixel(50, 50), WHITE);
            assert_eq!(stats.rasters_drawn, 0);
            assert_eq!(stats.raster_pixels_embedded, 0);
        }

        // Hidden in the Reference Data panel.
        reference
            .find_raster_mut(id)
            .expect("the raster is there")
            .visible = false;
        {
            let source = PlanSource {
                reference: Some(&reference),
                ..PlanSource::default()
            };
            let (image, stats) = painted_source(&source, &frame, &paper, WHITE);
            assert_eq!(image.pixel(50, 50), WHITE);
            assert_eq!(stats.rasters_drawn, 0);
        }
        reference
            .find_raster_mut(id)
            .expect("the raster is there")
            .visible = true;

        // Shown, but a kilometre from the viewport: nothing of it is embedded.
        frame = frame_of(100.0, 100.0, 2.0, Point2::new(1050.0, 50.0), 0.0);
        let source = PlanSource {
            reference: Some(&reference),
            ..PlanSource::default()
        };
        let (image, stats) = painted_source(&source, &frame, &paper, WHITE);
        assert_eq!(image.pixel(50, 50), WHITE);
        assert_eq!(stats.rasters_drawn, 0);
        assert_eq!(stats.raster_pixels_embedded, 0);
    }

    #[test]
    fn a_paper_crop_is_resampled_once_and_the_crops_kept_are_bounded() {
        // The sheet editor paints paper, and repaints for a pan of the editor or a second viewport
        // over the same image: the same crop at the same size comes from the cache, and is drawn
        // exactly as when it was made. A crop elsewhere is made afresh; however many are made, no
        // more than MAXIMUM_RASTER_CROPS are kept.
        let mut reference = ReferenceData::new();
        reference.add(raster_of(
            400,
            400,
            [0.0, 0.25, 0.0, 100.0, 0.0, -0.25],
            |x, y| Color::rgb((x % 256) as u8, (y % 256) as u8, 90),
        ));
        let source = PlanSource {
            reference: Some(&reference),
            ..PlanSource::default()
        };
        let settings = PlotSettings::default();
        let paper = PlanPaintOptions {
            raster_dpi_cap: 100.0,
            ..paper_options(&settings, 8.0)
        };
        let mut cache = PlanPaintCache::default();
        let frame = frame_of(100.0, 80.0, 4.0, Point2::new(50.0, 50.0), 0.0);
        let (first, stats) = painted_source_with(&source, &frame, &paper, WHITE, &mut cache);
        assert_eq!(stats.raster_crops_made, 1);
        let (again, stats) = painted_source_with(&source, &frame, &paper, WHITE, &mut cache);
        assert_eq!(stats.raster_crops_made, 0);
        assert_eq!(stats.rasters_drawn, 1);
        assert!(first == again);
        assert_eq!(cache.raster_crop_count(), 1);

        for i in 0..12 {
            let elsewhere = frame_of(
                100.0,
                80.0,
                4.0,
                Point2::new(20.0 + 5.0 * f64::from(i), 40.0),
                0.0,
            );
            let (_, stats) = painted_source_with(&source, &elsewhere, &paper, WHITE, &mut cache);
            assert_eq!(stats.raster_crops_made, 1, "{i}");
            assert!(cache.raster_crop_count() <= PlanPaintCache::MAXIMUM_RASTER_CROPS);
        }
        assert_eq!(
            cache.raster_crop_count(),
            PlanPaintCache::MAXIMUM_RASTER_CROPS
        );
        cache.invalidate_references();
        assert_eq!(cache.raster_crop_count(), 0);
    }

    #[test]
    fn a_point_cloud_on_paper_fills_the_turned_viewport_at_the_capped_resolution() {
        // Red points every half metre over (-60, -60)-(60, 60), under a 200 x 100 frame at 2 px a
        // metre turned 30 degrees, on paper at 8 px/mm with imagery capped at 100 dpi: 0.4921 of
        // the device's resolution, so the splat's image is 0.9843 px a metre. It covers the box
        // around the turned viewport - half-widths of 111.6 and 93.3 px, 55.8 and 46.65 m - where
        // that box meets the cloud: 111.6 x 93.3 m, which with a pixel of margin either side is
        // ceil(109.8 + 2) x ceil(91.8 + 2) = 112 x 94 pixels. Every corner of the device is inside
        // the turned viewport and must be red: the screen's splat, sized to the unturned view,
        // leaves those corners empty. With a pixel cap of 2 000 it is at most that.
        //
        // Uncapped, the image is no finer than CLOUD_POINT_PAPER_MILLIMETRES = 0.25 mm a pixel,
        // 4 px/mm, so that a point is the same size on the page at every resolution: the same
        // sheet at 8 px/mm (a frame of 2 px a metre) and at 16 (4 px a metre) splats into the same
        // image, 1 px a metre, ceil(111.6 + 2) x ceil(93.3 + 2) = 114 x 96.
        let mut cloud = PointCloudLayer {
            color_mode: PointColorMode::SourceColor,
            ..PointCloudLayer::default()
        };
        for i in 0..=240 {
            for j in 0..=240 {
                cloud.points.push(PointCloudPoint {
                    x: -60.0 + 0.5 * f64::from(i),
                    y: -60.0 + 0.5 * f64::from(j),
                    red: 220,
                    has_color: true,
                    ..PointCloudPoint::default()
                });
            }
        }
        cloud.bounds.min_x = -60.0;
        cloud.bounds.min_y = -60.0;
        cloud.bounds.max_x = 60.0;
        cloud.bounds.max_y = 60.0;
        cloud.bounds.min_z = 0.0;
        cloud.bounds.max_z = 0.0;
        cloud.source_point_count = cloud.points.len() as u64;
        let mut reference = ReferenceData::new();
        reference.add(cloud);
        let source = PlanSource {
            reference: Some(&reference),
            ..PlanSource::default()
        };
        let settings = PlotSettings::default();
        let mut paper = PlanPaintOptions {
            raster_dpi_cap: 100.0,
            ..paper_options(&settings, 8.0)
        };
        let frame = frame_of(200.0, 100.0, 2.0, Point2::new(0.0, 0.0), PI / 6.0);
        let (printed, stats) = painted_source(&source, &frame, &paper, WHITE);
        assert!(stats.cloud_points_drawn > 0);
        assert_near(
            stats.cloud_pixels_embedded as f64,
            112.0 * 94.0,
            112.0 * 3.0,
        );
        for (x, y) in [(1, 1), (198, 1), (1, 98), (198, 98), (100, 50)] {
            assert!(
                near(printed.pixel(x, y), Color::rgb(220, 0, 0), 2),
                "{x},{y}"
            );
        }

        paper.raster_pixel_cap = 2_000;
        let (coarse, stats) = painted_source(&source, &frame, &paper, WHITE);
        assert!(stats.cloud_pixels_embedded <= 2_000);
        assert!(stats.cloud_pixels_embedded > 1_500);
        assert!(near(coarse.pixel(100, 50), Color::rgb(220, 0, 0), 2));

        let uncapped = |k: f64| {
            let sheet = PlanPaintOptions {
                raster_dpi_cap: 0.0,
                ..paper_options(&settings, 8.0 * k)
            };
            painted_source(
                &source,
                &frame_of(
                    200.0 * k,
                    100.0 * k,
                    2.0 * k,
                    Point2::new(0.0, 0.0),
                    PI / 6.0,
                ),
                &sheet,
                WHITE,
            )
            .1
            .cloud_pixels_embedded
        };
        assert_eq!(uncapped(1.0), 114 * 96);
        assert_eq!(uncapped(2.0), 114 * 96);
    }

    #[test]
    fn a_mesh_footprint_on_paper_is_outlined_in_paper_millimetres_at_every_resolution() {
        // A black mesh 60 units square about the origin, plotted twice: at 10 px/mm through a
        // frame of 1 px a unit, and at 20 px/mm through one of 2 px a unit - the same sheet at
        // twice the resolution. Its outline is MESH_OUTLINE_PAPER_MILLIMETRES = 0.13 mm, 1.3 and
        // 2.6 px, half of it outside the square, dashed 2 mm on and 1 mm off; inside is the faint
        // fill. The ink OUTSIDE the square is the outline's outer half alone, so at twice the
        // resolution, twice as long and twice as wide, it is four times as much. A screen's
        // one-pixel pen would be twice as much.
        let mesh = TriangleMesh {
            vertices: vec![
                Point3::new(-30.0, -30.0, 0.0),
                Point3::new(30.0, -30.0, 0.0),
                Point3::new(30.0, 30.0, 0.0),
                Point3::new(-30.0, 30.0, 0.0),
            ],
            faces: vec![[0, 1, 2], [0, 2, 3]],
        };
        let meshes = vec![SceneMesh {
            name: String::from("slab"),
            mesh: Some(&mesh),
            flat_color: katana_render::rgba(0, 0, 0),
            ..SceneMesh::default()
        }];
        let source = PlanSource {
            meshes: Some(&meshes),
            ..PlanSource::default()
        };
        let settings = PlotSettings::default();
        let outside_ink = |k: f64| {
            let (image, _) = painted_source(
                &source,
                &frame_of(100.0 * k, 100.0 * k, k, Point2::new(0.0, 0.0), 0.0),
                &paper_options(&settings, 10.0 * k),
                WHITE,
            );
            let lo = (20.0 * k) as u32;
            let hi = (80.0 * k) as u32;
            let mut total = 0.0;
            for y in 0..image.height() {
                for x in 0..image.width() {
                    if x < lo || x >= hi || y < lo || y >= hi {
                        total += f64::from(255 - grey(image.pixel(x, y))) / 255.0;
                    }
                }
            }
            total
        };
        let coarse = outside_ink(1.0);
        let fine = outside_ink(2.0);
        assert!(coarse > 10.0, "{coarse}");
        assert!(
            fine / coarse > 3.4 && fine / coarse < 4.6,
            "{}",
            fine / coarse
        );
    }

    #[test]
    fn a_single_page_plot_embeds_the_raster_it_shows_and_not_a_hidden_one() {
        // File > Plot to PDF: the imagery prints, as one embedded image, and a raster hidden in
        // the view does not.
        let mut reference = ReferenceData::new();
        let id = reference.add(raster_of(
            200,
            200,
            [0.0, 1.0, 0.0, 200.0, 0.0, -1.0],
            |x, y| Color::rgb(x as u8, y as u8, 128),
        ));
        let source = PlanSource {
            reference: Some(&reference),
            ..PlanSource::default()
        };
        let settings = PlotSettings {
            paper: PaperSize::A4,
            dpi: 150.0,
            scale_denominator: 2000.0,
            center: Point2::new(100.0, 100.0),
            ..PlotSettings::default()
        };
        let scratch = Scratch::new("a_single_page_plot_embeds_the_raster_it_shows");
        let plotted = |hidden: &BTreeSet<u64>| {
            let path = scratch.path("plot.pdf");
            let mut cache = PlanPaintCache::default();
            let status = plot_plan_to_pdf(
                &path,
                &settings,
                &source,
                &LayerOverrides::default(),
                hidden,
                &mut cache,
                "",
                None,
                pdf_writer().as_mut(),
            );
            assert!(status.is_ok());
            std::fs::read(&path).expect("the PDF is read")
        };
        // An image XObject; every page's resources name the image kinds whether it has one or
        // not.
        let has_image = |bytes: &[u8]| {
            let needle = b"/Subtype /Image";
            bytes.windows(needle.len()).any(|window| window == needle)
        };
        assert!(has_image(&plotted(&BTreeSet::new())));
        assert!(!has_image(&plotted(&BTreeSet::from([id]))));
    }

    #[test]
    fn a_single_page_plot_hands_back_what_it_is_without_of_a_surfaces_style_and_is_written_all_the_same()
     {
        // The 10 x 10 square z = x as the surface "ground", in a style whose contours are a
        // millionth of a unit apart: each of its two triangles runs from 0 to 10, so ten million
        // levels cross each, past the four million the plan allows. The plot of it at 1:100 about
        // (5, 5) on A4 (a 277 x 190 mm drawing area, 27.7 x 19 m) shows all of it, and is without
        // its contours; the caller is told which surface, which style and why.
        let mut store = SurfaceStore::new();
        store
            .add(NamedSurface {
                name: String::from("ground"),
                surface: Some(square_tin()),
                source: String::from("test"),
                style: String::from("Fine"),
                visible: true,
            })
            .expect("the surface is added");
        let mut styles = built_in_surface_styles();
        let mut fine = SurfaceDisplayStyle {
            name: String::from("Fine"),
            ..SurfaceDisplayStyle::default()
        };
        fine.contours.show = true;
        fine.contours.interval = 1.0e-6;
        styles.styles.push(fine);
        let settings = PlotSettings {
            paper: PaperSize::A4,
            dpi: 150.0,
            scale_denominator: 100.0,
            center: Point2::new(5.0, 5.0),
            ..PlotSettings::default()
        };
        let scratch = Scratch::new("a_single_page_plot_hands_back_what_it_is_without");
        let path = scratch.path("plot.pdf");

        // The list the caller holds is replaced, so a stale line does not stay.
        let mut problems = vec![SurfaceProblem {
            surface: String::from("stale"),
            style: String::from("old"),
            reason: String::from("from an earlier plot"),
        }];
        {
            let source = PlanSource {
                surfaces: Some(&store),
                surface_styles: Some(&styles),
                ..PlanSource::default()
            };
            let mut cache = PlanPaintCache::default();
            plot_plan_to_pdf(
                &path,
                &settings,
                &source,
                &LayerOverrides::default(),
                &BTreeSet::new(),
                &mut cache,
                "",
                Some(&mut problems),
                pdf_writer().as_mut(),
            )
            .expect("the plot is written");
        }
        assert_eq!(problems.len(), 1);
        assert_eq!(problems[0].surface, "ground");
        assert_eq!(problems[0].style, "Fine");
        assert_eq!(
            problems[0].reason,
            "contours: an interval of 0.000001 crosses this surface's triangles more than 4000000 \
             times; not drawn - set a larger interval"
        );
        assert!(written(&path), "the plot is written all the same");

        // The same plot of a style that can be drawn lost nothing, and asking for the list is
        // optional.
        if let Some(last) = styles.styles.last_mut() {
            last.contours.interval = 1.0;
        }
        let source = PlanSource {
            surfaces: Some(&store),
            surface_styles: Some(&styles),
            ..PlanSource::default()
        };
        let mut again = PlanPaintCache::default();
        plot_plan_to_pdf(
            &path,
            &settings,
            &source,
            &LayerOverrides::default(),
            &BTreeSet::new(),
            &mut again,
            "",
            Some(&mut problems),
            pdf_writer().as_mut(),
        )
        .expect("the plot is written");
        assert!(problems.is_empty());
        let mut unasked = PlanPaintCache::default();
        assert!(
            plot_plan_to_pdf(
                &path,
                &settings,
                &source,
                &LayerOverrides::default(),
                &BTreeSet::new(),
                &mut unasked,
                "",
                None,
                pdf_writer().as_mut(),
            )
            .is_ok()
        );
    }

    // ---- screen speed, without a change of look ----

    #[test]
    fn a_line_clipped_to_the_view_paints_what_the_whole_line_paints_to_within_antialiasing() {
        // A zig-zag of 400 vertices from x = -2000 to 2000 through a 200 x 100 view about (3, 1)
        // at 1 px a unit: all but a twentieth of it is off screen. Where it crosses the view
        // (i = 190 to 210) its even vertices are at y = -80 + 0.37 i, about -10 to -2, inside,
        // and its odd ones at about 150, far above: it leaves the view and comes back ten times.
        // Clipped with the pen's margin it must paint what the whole line paints, at the 1.5 px
        // hairline and the thin pixel both.
        let mut model = Model::new();
        let vertices = (0..400)
            .map(|i| {
                Point2::new(
                    -2000.0 + 10.0 * f64::from(i),
                    (if i % 2 == 0 { -80.0 } else { 80.0 }) + 0.37 * f64::from(i),
                )
            })
            .collect();
        add(
            &mut model,
            entity_of(Geometry::Polyline(Polyline2::new(vertices, false)), "0"),
        );
        let frame = frame_of(200.0, 100.0, 1.0, Point2::new(3.0, 1.0), 0.0);
        for thin in [false, true] {
            let whole = PlanPaintOptions {
                clip_lines: false,
                thin_lines: thin,
                ..screen()
            };
            let clipped = PlanPaintOptions {
                clip_lines: true,
                ..whole.clone()
            };
            let (expected, _) = painted(&model, &frame, &whole, BLACK);
            let (actual, stats) = painted(&model, &frame, &clipped, BLACK);
            assert_eq!(stats.lines_clipped, 1);
            let mut differing = 0;
            let mut largest = 0;
            for y in 0..expected.height() {
                for x in 0..expected.width() {
                    let difference = (grey(expected.pixel(x, y)) - grey(actual.pixel(x, y))).abs();
                    differing += i32::from(difference > 0);
                    largest = largest.max(difference);
                }
            }
            // Measured in the C++: 1 257 pixels a level or three apart for the 1.5 px pen, 94 up
            // to eight levels apart along the view's edge for the thin one. A rasteriser is not
            // local to the segment: the same segment drawn in a shorter polyline comes out a few
            // levels of antialiasing different, which no eye sees and a bit-for-bit comparison
            // does. Anything more - a gap, a missing run, a line drawn where it was not - is
            // dozens of levels.
            assert!(
                largest <= 8,
                "thin {thin}, {differing} pixels differ, by up to {largest}"
            );
        }
    }

    #[test]
    fn each_layer_and_style_is_resolved_once_per_paint_however_many_entities_share_it() {
        // 100 lines on layer "a" and 50 on "b", no styles or colours of their own: two displays
        // to resolve, not 150.
        let mut model = Model::new();
        for name in ["a", "b"] {
            model
                .layers
                .add(Layer {
                    name: String::from(name),
                    ..Layer::default()
                })
                .expect("the layer is added");
        }
        for i in 0..150 {
            let y = f64::from(i) * 0.5 - 37.0;
            add(
                &mut model,
                entity_of(
                    Geometry::Line(Segment2::new(Point2::new(-40.0, y), Point2::new(40.0, y))),
                    if i < 100 { "a" } else { "b" },
                ),
            );
        }
        let (_, stats) = painted(
            &model,
            &frame_of(100.0, 100.0, 1.0, Point2::new(0.0, 0.0), 0.0),
            &screen(),
            BLACK,
        );
        assert_eq!(stats.entities_drawn, 150);
        assert_eq!(stats.displays_resolved, 2);
    }

    #[test]
    fn thin_screen_lines_are_one_pixel_and_the_hairline_is_wider() {
        // The line y = -0.5 in a 100-pixel-high view about the origin at 1 px a unit lies along
        // y = 50.5, the middle of pixel row 50. A cosmetic pixel pen covers row 50 alone; the
        // 1.5 px hairline covers 49.75 to 51.25, a quarter of rows 49 and 51 as well.
        let mut model = Model::new();
        add(
            &mut model,
            entity_of(
                Geometry::Line(Segment2::new(
                    Point2::new(-40.0, -0.5),
                    Point2::new(40.0, -0.5),
                )),
                "0",
            ),
        );
        let frame = frame_of(100.0, 100.0, 1.0, Point2::new(0.0, 0.0), 0.0);
        let thin = PlanPaintOptions {
            thin_lines: true,
            ..screen()
        };
        let (one, _) = painted(&model, &frame, &thin, BLACK);
        assert_ne!(one.pixel(50, 50), BLACK);
        assert_eq!(one.pixel(50, 49), BLACK);
        assert_eq!(one.pixel(50, 51), BLACK);
        let (hairline, _) = painted(&model, &frame, &screen(), BLACK);
        assert_ne!(hairline.pixel(50, 49), BLACK);
        assert_ne!(hairline.pixel(50, 51), BLACK);
    }

    #[test]
    fn symbols_are_stamped_from_sprites_on_screen_and_stroked_on_paper() {
        // Forty points in a style drawing the built-in "circle" 6 units wide, at 2 px a unit (12
        // px across): on screen each stamp is a sprite, and the stamps share at most one sprite
        // per quarter-pixel phase (4 x 4); drawn from the sprites the points look as stroked ones
        // do, within an eighth of a pixel's antialiasing. On paper every stamp is stroked.
        let mut model = Model::new();
        model
            .styles
            .add(Style {
                name: String::from("manhole"),
                symbol: String::from("circle"),
                symbol_size: 6.0,
                ..Style::default()
            })
            .expect("the style is added");
        for i in 0..40 {
            let mut point = entity_of(
                Geometry::Point(PointGeometry {
                    position: Point2::new(-45.0 + 2.3 * f64::from(i), 3.0 + 0.1 * f64::from(i)),
                }),
                "0",
            );
            point.style = String::from("manhole");
            add(&mut model, point);
        }
        let frame = frame_of(240.0, 120.0, 2.0, Point2::new(0.0, 0.0), 0.0);
        let sprites = PlanPaintOptions {
            symbol_sprites: true,
            ..screen()
        };
        let mut cache = PlanPaintCache::default();
        let (from_sprites, sprite_stats) =
            painted_with(&model, &frame, &sprites, BLACK, &mut cache);
        assert_eq!(sprite_stats.symbols_stamped, 40);
        assert_eq!(sprite_stats.sprites_drawn, 40);
        assert!(cache.sprite_count() <= 16);
        let (vector, _) = painted(&model, &frame, &screen(), BLACK);
        // Every stamp in the same place: nowhere does one image have ink where the other has none
        // by more than faint antialiasing.
        let mut strong_differences = 0;
        for y in 0..vector.height() {
            for x in 0..vector.width() {
                if (grey(vector.pixel(x, y)) - grey(from_sprites.pixel(x, y))).abs() > 96 {
                    strong_differences += 1;
                }
            }
        }
        assert_eq!(strong_differences, 0);

        let settings = PlotSettings::default();
        let paper = PlanPaintOptions {
            symbol_sprites: true, // asked for, and not given: a plot stays vector
            ..paper_options(&settings, 4.0)
        };
        let (_, paper_stats) = painted(&model, &frame, &paper, WHITE);
        assert_eq!(paper_stats.symbols_stamped, 40);
        assert_eq!(paper_stats.sprites_drawn, 0);
    }

    // ---- what the cache keeps across paints ----

    #[test]
    fn a_linetype_edited_between_two_paints_sharing_a_cache_is_drawn_with_its_new_dashes() {
        // A segment x = -85 to 85 at 1 px a unit in a view 200 px wide about the origin runs from
        // column 15 to 185 along row 20, on a layer whose model linetype is 20 on, 20 off: dashes
        // over columns 15-35, 55-75, 95-115, 135-155 and 175-185 (the last cut short by the end),
        // five runs of ink (a square cap adds at most 0.75 px to each end, and the gaps are 20
        // px). The linetype is then edited to 10 on, 10 off - what the Style Manager's
        // update_linetype does to the model - and painted again through the SAME cache at the
        // same scale: 15-25, 35-45, ..., 175-185, nine runs. (170 is not a whole number of either
        // period, so no dash begins at the very end, where a cosmetic pen's last pixel would draw
        // one.) A dash pattern kept by the linetype's name drew the old five until the view was
        // zoomed, and a second plot at the same scale printed them.
        let mut model = Model::new();
        let mut fence = Linetype {
            name: String::from("fence"),
            pattern: vec![
                LinetypeElement { length: 20.0 },
                LinetypeElement { length: -20.0 },
            ],
            ..Linetype::default()
        };
        model
            .linetypes
            .add(fence.clone())
            .expect("the linetype is added");
        model
            .layers
            .add(Layer {
                name: String::from("fences"),
                color: EntityColor {
                    r: 255,
                    g: 255,
                    b: 255,
                    a: 255,
                },
                linetype: String::from("fence"),
                ..Layer::default()
            })
            .expect("the layer is added");
        add(
            &mut model,
            entity_of(
                Geometry::Line(Segment2::new(
                    Point2::new(-85.0, 0.0),
                    Point2::new(85.0, 0.0),
                )),
                "fences",
            ),
        );
        let frame = frame_of(200.0, 40.0, 1.0, Point2::new(0.0, 0.0), 0.0);
        for thin in [false, true] {
            let options = PlanPaintOptions {
                thin_lines: thin,
                ..screen()
            };
            let mut kept = PlanPaintCache::default();
            fence.pattern = vec![
                LinetypeElement { length: 20.0 },
                LinetypeElement { length: -20.0 },
            ];
            model
                .linetypes
                .update(&fence)
                .expect("the linetype is updated");
            let (first, _) = painted_with(&model, &frame, &options, BLACK, &mut kept);
            assert_eq!(runs_along(&first, 20, BLACK), 5, "thin {thin}");
            fence.pattern = vec![
                LinetypeElement { length: 10.0 },
                LinetypeElement { length: -10.0 },
            ];
            model
                .linetypes
                .update(&fence)
                .expect("the linetype is updated");
            let (again, _) = painted_with(&model, &frame, &options, BLACK, &mut kept);
            assert_eq!(runs_along(&again, 20, BLACK), 9, "thin {thin}");
            // And exactly what a cache that never saw the old pattern paints.
            assert!(
                again == painted(&model, &frame, &options, BLACK).0,
                "thin {thin}"
            );
        }
    }

    // ---- off the GUI thread ----

    #[test]
    fn two_threads_painting_the_same_drawing_at_once_each_paint_what_the_gui_thread_paints() {
        // The painter reads its arguments and writes only its canvas and its cache, so two worker
        // threads, each with its own cache, painting the same model at the same time onto their
        // own images - what a sheet set plotted in the background does - must each get exactly
        // the GUI thread's image: lines, a hatched square, text and sprite-stamped symbols.
        let mut model = Model::new();
        model
            .styles
            .add(Style {
                name: String::from("pit"),
                symbol: String::from("square"),
                symbol_size: 4.0,
                ..Style::default()
            })
            .expect("the style is added");
        for i in 0..30 {
            let i = f64::from(i);
            add(
                &mut model,
                entity_of(
                    Geometry::Line(Segment2::new(
                        Point2::new(-90.0 + 6.0 * i, -40.0),
                        Point2::new(-60.0 + 4.0 * i, 40.0),
                    )),
                    "0",
                ),
            );
            let mut point = entity_of(
                Geometry::Point(PointGeometry {
                    position: Point2::new(-80.0 + 5.3 * i, 10.0),
                }),
                "0",
            );
            point.style = String::from("pit");
            add(&mut model, point);
        }
        add(
            &mut model,
            entity_of(
                Geometry::Text(TextGeometry {
                    position: Point2::new(-50.0, -20.0),
                    text: String::from("LOT 7"),
                    height: 12.0,
                    rotation: 0.2,
                    ..TextGeometry::default()
                }),
                "0",
            ),
        );
        let frame = frame_of(200.0, 100.0, 1.0, Point2::new(0.0, 0.0), 0.0);
        let options = PlanPaintOptions {
            symbol_sprites: true,
            ..screen()
        };
        let expected = painted(&model, &frame, &options, BLACK).0;
        let (first, second) = std::thread::scope(|scope| {
            let one = scope.spawn(|| painted(&model, &frame, &options, BLACK).0);
            let two = scope.spawn(|| painted(&model, &frame, &options, BLACK).0);
            (
                one.join().expect("the first thread paints"),
                two.join().expect("the second thread paints"),
            )
        });
        assert!(first == expected);
        assert!(second == expected);
    }

    // ---- the selection's ghosts (cad's selection_style) ----

    #[test]
    fn a_selected_entity_on_a_layer_this_view_hides_is_drawn_as_a_ghost() {
        // The ghost: #FF9F1C at alpha 153, in 2 px square dots every 6 px from the line's start,
        // not antialiased (selection_style; the painter's dot_path). The 80 px run from column 10
        // has a dot at t = 0, 6 ... 78 along it, fourteen, and a dot about x covers columns x - 1
        // and x of rows 49 and 50, so row 50 has 28 pixels of ink - columns 9 and 10, then every
        // 6 - and nothing between. Ink is measured against a baseline, the same frame painted
        // with nothing selected, never as pixels lit in the ghosted image alone. Over black a
        // covered pixel is 153/255 of the orange: (153, 95.4, 16.8), within 2 of that after the
        // 8-bit premultiplied blend's rounding.
        let plan = GhostPlan::new();
        let (bare, unselected) = plan.paint(false);
        let (ghosted, selected) = plan.paint(true);

        assert_eq!(selected.ghosts_drawn, 1);
        assert_eq!(unselected.ghosts_drawn, 0);
        assert_eq!(
            selected.entities_drawn, unselected.entities_drawn,
            "a ghost is not an entity drawn"
        );
        assert_eq!(
            selected.entities_drawn, 0,
            "the layer is hidden in this view"
        );
        assert_eq!(orange_ink(&bare), 0);
        assert!(orange_ink(&ghosted) > orange_ink(&bare));

        let inked = |x: u32, y: u32| ghosted.pixel(x, y) != bare.pixel(x, y);
        let mut ink = 0;
        let mut exact = 0;
        for x in 0..100 {
            let c = ghosted.pixel(x, 50);
            ink += i32::from(inked(x, 50));
            exact += i32::from(
                inked(x, 50)
                    && (i32::from(c.r) - 153).abs() <= 2
                    && (i32::from(c.g) - 95).abs() <= 2
                    && (i32::from(c.b) - 17).abs() <= 2,
            );
        }
        assert_eq!(ink, 28, "fourteen dots, 2 px each");
        assert_eq!(exact, 28, "each the selection colour at 60 % over black");
        for dot in 0..14 {
            let x = 10 + 6 * dot;
            assert!(inked(x - 1, 50), "dot {dot}");
            assert!(inked(x, 50), "dot {dot}");
        }
    }

    #[test]
    fn no_ghost_where_the_document_hides_the_layer() {
        // A layer the drawing switches off stays off in every view: a ghost says "selected, but
        // hidden in this view", never "selected, though off".
        let mut plan = GhostPlan::new();
        let mut design = plan
            .model
            .layers
            .find("design")
            .expect("the layer is there")
            .clone();
        design.visible = false;
        plan.model
            .layers
            .update(&design)
            .expect("the layer is updated");
        let (image, stats) = plan.paint(true);
        assert_eq!(stats.ghosts_drawn, 0);
        assert_eq!(orange_ink(&image), 0);
    }

    #[test]
    fn no_ghost_of_an_entity_made_invisible() {
        // The same rule for the entity's own switch (cad's is_ghost): invisible, it is hidden in
        // every view, selected or not.
        let mut plan = GhostPlan::new();
        let id = plan.selection.ids()[0];
        let mut hidden = plan
            .model
            .entities
            .find(id)
            .expect("the entity is there")
            .clone();
        hidden.visible = false;
        plan.model
            .entities
            .replace(hidden)
            .expect("the entity is replaced");
        let (image, stats) = plan.paint(true);
        assert_eq!(stats.ghosts_drawn, 0);
        assert_eq!(orange_ink(&image), 0);
    }

    #[test]
    fn no_ghost_on_paper() {
        let mut plan = GhostPlan::new();
        static SETTINGS: std::sync::OnceLock<PlotSettings> = std::sync::OnceLock::new();
        let settings = SETTINGS.get_or_init(PlotSettings::default);
        plan.options = PlanPaintOptions {
            selection_ghosts: true,
            ..paper_options(settings, 4.0)
        };
        let (_, stats) = plan.paint(true);
        assert_eq!(
            stats.ghosts_drawn, 0,
            "a plot draws what the drawing shows, never a ghost"
        );
    }

    #[test]
    fn no_ghost_with_ghosts_off() {
        // PlanPaintOptions::selection_ghosts defaults to false, so every caller but the plan view
        // - which sets it from the view state's selection_ghosts - is unchanged.
        let mut plan = GhostPlan::new();
        plan.options = screen();
        assert!(!plan.options.selection_ghosts);
        let (image, stats) = plan.paint(true);
        assert_eq!(stats.ghosts_drawn, 0);
        assert_eq!(orange_ink(&image), 0);
    }

    #[test]
    fn a_selected_entity_the_view_shows_is_drawn_selected_and_not_as_a_ghost() {
        // The view hiding nothing: the line is drawn, in the selection's 2 px orange dashes, once.
        let mut plan = GhostPlan::new();
        plan.hidden_in_view = false;
        let (image, stats) = plan.paint(true);
        assert_eq!(stats.ghosts_drawn, 0);
        assert_eq!(stats.entities_drawn, 1);
        assert!(orange_ink(&image) > 0);
    }

    #[test]
    fn a_ghost_longer_than_the_view_is_dotted_across_it_from_its_own_start() {
        // A line from (-1e6, 50) to (1e6, 50), both ends far outside the frame, which shows x from
        // 0 to 100 at 1 px a unit: the dots are laid only where it crosses the view, and in step
        // with the line's own start. A dot every 6 px from x = -1e6 puts one at every
        // x = 6k - 1e6, and -1e6 = 2 - 6 x 166 667, so at x = 2, 8, 14 ... 98: 17 dots. A dot 2 px
        // square about x covers the columns x - 1 and x, so columns 1 and 2 are lit and 3 to 6
        // are not.
        let mut plan = GhostPlan::new();
        let id = add(
            &mut plan.model,
            entity_of(
                Geometry::Line(Segment2::new(
                    Point2::new(-1.0e6, 50.0),
                    Point2::new(1.0e6, 50.0),
                )),
                "design",
            ),
        );
        plan.selection.set(vec![id]);
        let (image, stats) = plan.paint(true);
        assert_eq!(stats.ghosts_drawn, 1);
        // Ink against the baseline, the frame with nothing selected.
        let (bare, _) = plan.paint(false);
        let lit = |x: u32| image.pixel(x, 50) != bare.pixel(x, 50);
        assert!(lit(1));
        assert!(lit(2));
        for x in 3..=6 {
            assert!(!lit(x), "column {x}");
        }
        assert!(lit(7));
        assert!(lit(8));
        let columns = (0..100).filter(|&x| lit(x)).count();
        assert_eq!(columns, 17 * 2);
    }

    // ---- a ghost of each kind of geometry ----
    //
    // draw_selection_ghosts has a branch for each kind: a line is dotted as it is; a polyline, an
    // arc or a circle goes through draw_geometry's own stroke_polyline, dotted only because the
    // ghost pass asks it to be; a point is a ring; a text or a dimension the outline of the box
    // it draws; a label nothing. Each is painted alone, selected, on the layer GhostPlan's view
    // hides, and its ink read against the same frame with nothing selected.

    #[test]
    fn a_ghosted_polyline_is_dotted_along_each_leg_with_its_step_carried_round_the_corner() {
        // (10,50) -> (40,50) -> (40,20) in the model is screen (10,50) -> (40,50) -> (40,80) in
        // the 100 x 100 frame at 1 px a unit about (50, 50). A dot every 6 px from the start: at
        // 0, 6 ... 24 along the 30 px first leg, x = 10, 16, 22, 28 and 34 on row 50; 30 is a
        // whole number of steps, so the next falls on the corner, (40, 50); then y = 56, 62, 68
        // and 74 down the second leg, whose end, 30 along it, is not dotted (a dot is laid short
        // of a leg's end). A 2 px dot about (x, y) covers columns x - 1 and x of rows y - 1 and y:
        // ten dots, forty pixels, and nothing between them - where the solid stroke draw_geometry
        // lays without the dots covers row 50 from column 9 to 40.
        let mut plan = GhostPlan::new();
        select_only(
            &mut plan,
            Geometry::Polyline(Polyline2::new(
                vec![
                    Point2::new(10.0, 50.0),
                    Point2::new(40.0, 50.0),
                    Point2::new(40.0, 20.0),
                ],
                false,
            )),
        );
        let (ink, stats) = ghost_ink(&plan);
        assert_eq!(stats.ghosts_drawn, 1);
        assert_eq!(stats.entities_drawn, 0);

        let mut expected = Vec::new();
        let mut dot = |x: i32, y: i32| {
            expected.extend([(x - 1, y - 1), (x, y - 1), (x - 1, y), (x, y)]);
        };
        for x in [10, 16, 22, 28, 34, 40] {
            dot(x, 50);
        }
        for y in [56, 62, 68, 74] {
            dot(40, y);
        }
        assert_eq!(
            sorted(ink),
            sorted(expected),
            "ten 2 px dots and nothing between them"
        );
    }

    #[test]
    fn a_ghosted_circle_is_a_row_of_separate_dots_on_its_curve() {
        // A circle of radius 30 about (50, 50): screen radius 30 about (50, 50), chorded to a
        // quarter of a pixel and dotted every 6 px of its 188.5 px round, 32 dots (the last 2.5 px
        // short of the first, so the two may touch). A dot is put on the whole pixel nearest where
        // it falls, at most half a pixel off each way, and its 2 px square covers the two pixel
        // centres half a pixel either side of that: a pixel of ink is at most a pixel off a point
        // on a chord each way, 1.42 px in all, and a chord is within 0.25 px of the circle - so
        // 28.33 to 31.42 px from the centre. Without the dots, the ghost pen strokes the circle
        // solid: one mark, not thirty.
        let mut plan = GhostPlan::new();
        select_only(
            &mut plan,
            Geometry::Circle(Circle2::new(Point2::new(50.0, 50.0), 30.0)),
        );
        let (ink, stats) = ghost_ink(&plan);
        assert_eq!(stats.ghosts_drawn, 1);
        for &(x, y) in &ink {
            let r = (f64::from(x) + 0.5 - 50.0).hypot(f64::from(y) + 0.5 - 50.0);
            assert!(r >= 28.33, "{x},{y}");
            assert!(r <= 31.42, "{x},{y}");
        }
        let marks = marks_in(&ink);
        assert!(marks >= 31, "a dot every 6 px: {marks}");
        assert!(marks <= 32, "{marks}");
    }

    #[test]
    fn a_ghosted_point_is_a_dotted_ring_around_it_not_a_mark_on_it() {
        // A ring of 6 px radius about the point (selection_style), outside the cross the point is
        // drawn with: a 24-gon whose round is 37.6 px, dotted every 6 px - seven dots, the last
        // 1.6 px short of the first, so six marks or seven. A pixel of ink is at most 1.42 px from
        // a point on the ring's chords (as the circle's above), which are 5.95 to 6 px out: 4.53
        // to 7.42 px from the centre, and nothing on the point itself.
        let mut plan = GhostPlan::new();
        select_only(
            &mut plan,
            Geometry::Point(PointGeometry {
                position: Point2::new(50.0, 50.0),
            }),
        );
        let (ink, stats) = ghost_ink(&plan);
        assert_eq!(stats.ghosts_drawn, 1);
        assert!(!ink.is_empty());
        for &(x, y) in &ink {
            let r = (f64::from(x) + 0.5 - 50.0).hypot(f64::from(y) + 0.5 - 50.0);
            assert!(r >= 4.53, "{x},{y}");
            assert!(r <= 7.42, "{x},{y}");
        }
        let marks = marks_in(&ink);
        assert!((6..=7).contains(&marks), "{marks}");
    }

    #[test]
    fn a_ghosted_text_is_the_dotted_outline_of_its_box_not_its_letters() {
        // "LOT 7", 10 units tall from its baseline's left end at (20, 40). One line of
        // baseline-left text is its height tall above the baseline (text_block_extent), so its box
        // is x from 20, y from 40 to 50: screen column 20 on, rows 50 to 60, and wider than it is
        // tall. A dot about an edge covers the pixel before it as well, so the ink runs from
        // column 19 and over rows 49 to 60. Its words at a ghost's strength would be unreadable,
        // and letters would put ink inside the box.
        let mut plan = GhostPlan::new();
        select_only(
            &mut plan,
            Geometry::Text(TextGeometry {
                position: Point2::new(20.0, 40.0),
                text: String::from("LOT 7"),
                height: 10.0,
                rotation: 0.0,
                ..TextGeometry::default()
            }),
        );
        let (ink, stats) = ghost_ink(&plan);
        assert_eq!(stats.ghosts_drawn, 1);
        let (left, top, right, bottom) = expect_dotted_box(&ink);
        assert_eq!(left, 19);
        assert_eq!(top, 49);
        assert_eq!(bottom, 60);
        // QRect's width and height count the pixels at both edges.
        assert!(right - left + 1 > bottom - top + 1);
    }

    #[test]
    fn a_ghosted_dimension_is_the_dotted_outline_of_what_it_draws() {
        // From (20, 30) to (80, 30), 10 above: screen (20, 70) to (80, 70), its dimension line 10
        // px higher. The outline of the box the dimension draws - extension lines, arrows and
        // text - holds both points; drawn as itself, its dimension line would cross the box's
        // inside.
        let mut plan = GhostPlan::new();
        select_only(
            &mut plan,
            Geometry::Dimension(DimensionGeometry {
                start: Point2::new(20.0, 30.0),
                end: Point2::new(80.0, 30.0),
                offset: 10.0,
                ..DimensionGeometry::default()
            }),
        );
        let (ink, stats) = ghost_ink(&plan);
        assert_eq!(stats.ghosts_drawn, 1);
        let (left, top, right, bottom) = expect_dotted_box(&ink);
        assert!(left <= 21);
        assert!(right >= 79);
        assert!(bottom >= 69);
        assert!(
            top <= 60,
            "the dimension line and its text stand above the points"
        );
    }

    #[test]
    fn a_label_is_never_ghosted() {
        // A label is placed with the others, and has no outline of its own to dot: selected on a
        // layer the view hides, it leaves no trace there.
        let mut plan = GhostPlan::new();
        // Labelling the fixture's line, a label's one target.
        let line = plan.selection.ids()[0];
        select_only(
            &mut plan,
            Geometry::Label(LabelGeometry {
                target: line,
                style: String::from("any"),
                anchor: Point2::new(50.0, 50.0),
                ..LabelGeometry::default()
            }),
        );
        assert_eq!(plan.selection.size(), 1);
        assert_ne!(plan.selection.ids()[0], line, "the label alone is selected");
        let (ink, stats) = ghost_ink(&plan);
        assert_eq!(stats.ghosts_drawn, 0);
        assert!(ink.is_empty(), "{} pixels", ink.len());
    }

    #[test]
    fn at_a_fractional_scale_every_ghost_dot_is_the_same_square_of_whole_pixels() {
        // At 125 % - the owner's display (docs/render.md) - the 2 px pen was 2.5 device pixels,
        // drawn without antialiasing as 2 or 3: a straight ghost beaded 3, 2, 3 pixels, and a
        // slanted one mixed four shapes of dot. The pen and the pitch are whole device pixels
        // now, rounded - 2 x 1.25 = 2.5 to 3, 6 x 1.25 = 7.5 to 8 - and each dot is put on whole
        // pixels. GhostPlan's line, (10,50)-(90,50), is device (12.5, 62.5) to (112.5, 62.5): 100
        // device pixels, a dot at 0, 8 ... 96 along it, thirteen, each 3 px square about a
        // pixel's centre - columns 11 to 13 of rows 61 to 63 for the first, and 8 columns on for
        // each after. Ink against the frame painted with nothing selected, at the same scale.
        let plan = GhostPlan::new();
        let paint_at = |selected: bool| {
            let mut image = Image::new(125, 125);
            image.set_device_pixel_ratio(1.25);
            image.fill(BLACK);
            let none = SelectionSet::default();
            let source = PlanSource {
                model: Some(&plan.model),
                selection: Some(if selected { &plan.selection } else { &none }),
                ..PlanSource::default()
            };
            let mut cache = PlanPaintCache::default();
            let frame = plan.frame();
            paint_on(&mut image, |canvas| {
                paint_plan(canvas, &source, &frame, &plan.options, &mut cache);
            });
            image
        };
        let bare = paint_at(false);
        let ghosted = paint_at(true);
        let mut ink = Vec::new();
        for y in 0..ghosted.height() {
            for x in 0..ghosted.width() {
                if ghosted.pixel(x, y) != bare.pixel(x, y) {
                    ink.push((x as i32, y as i32));
                }
            }
        }
        let mut expected = Vec::new();
        for dot in 0..13 {
            for y in 61..=63 {
                for x in (11 + 8 * dot)..=(13 + 8 * dot) {
                    expected.push((x, y));
                }
            }
        }
        assert_eq!(ink.len(), expected.len());
        assert_eq!(
            sorted(ink),
            sorted(expected),
            "thirteen 3 x 3 dots, 8 px apart"
        );
    }
}
