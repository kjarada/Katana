//! The twins of tests/qt_widgets/plotting/test_legend_painter.cpp: a legend's samples as they
//! print (katana_window::plotting::legend_painter), and the panel on a sheet. Each sample is
//! painted into a blank image and its ink measured; the panel is read off a painted sheet.

use katana_cad::plot::{PlotColourMode, PlotSettings};
use katana_cad::plotting::legend::{LegendEntry, LegendKind};
use katana_cad::plotting::sheet_set::{LegendScope, Sheet, SheetSet, Viewport, ViewportKind};
use katana_entity::tables::CONTINUOUS_LINETYPE;
use katana_entity::{
    Color as EntityColor, Entity, Geometry, HatchPattern, Layer, LineStyle, Model, PointGeometry,
    Stroke, StrokeOp, Style, StyleLibrary, StyleUnits,
};
use katana_geometry::{Box2, Point2, Segment2};
use katana_window::canvas::{Color, Image, RectF, RenderHint};
use katana_window::plan_painter::PlanSource;
use katana_window::plotting::legend_painter::{LegendSampleContext, paint_legend_sample};
use katana_window::sheet_painter::SheetSource;

use crate::support::{PPMM, WHITE, bx, ink_in, near as within, paint_on, painted, pixels_in};

const RED: EntityColor = EntityColor {
    r: 200,
    g: 0,
    b: 0,
    a: 255,
};
const BLUE: EntityColor = EntityColor {
    r: 0,
    g: 0,
    b: 255,
    a: 255,
};

// ---- one sample ---------------------------------------------------------------------------------

/// A sample cell of the legend's 10 x 3.6 mm, centred at pixel (100, 50).
const CELL: RectF = RectF::new(80.0, 42.8, 10.0 * PPMM, 3.6 * PPMM);

fn blank() -> Image {
    let mut image = Image::new(200, 100);
    image.fill(WHITE);
    image
}

fn context<'a>(
    plot: &'a PlotSettings,
    model: Option<&'a Model>,
    library: Option<&'a StyleLibrary>,
) -> LegendSampleContext<'a> {
    LegendSampleContext {
        model,
        library,
        plot: Some(plot),
        pixels_per_millimetre: PPMM,
        scale: 500.0,
        ..LegendSampleContext::default()
    }
}

fn paint_sample(image: &mut Image, entry: &LegendEntry, c: &LegendSampleContext<'_>) -> bool {
    let mut drawn = false;
    paint_on(image, |canvas| {
        canvas.set_render_hint(RenderHint::Antialiasing, true);
        drawn = paint_legend_sample(canvas, entry, CELL, c);
    });
    drawn
}

/// The box of the pixels that are not white, and how many there are.
struct Ink {
    count: i32,
    left: i32,
    right: i32,
    top: i32,
    bottom: i32,
}

impl Ink {
    fn width(&self) -> i32 {
        if self.count == 0 {
            0
        } else {
            self.right - self.left + 1
        }
    }

    fn height(&self) -> i32 {
        if self.count == 0 {
            0
        } else {
            self.bottom - self.top + 1
        }
    }

    /// The box as a rectangle, for asking whether a cell holds it.
    fn rect(&self) -> RectF {
        RectF::new(
            f64::from(self.left),
            f64::from(self.top),
            f64::from(self.width()),
            f64::from(self.height()),
        )
    }
}

fn ink_of(image: &Image) -> Ink {
    let mut ink = Ink {
        count: 0,
        left: 1 << 20,
        right: -1,
        top: 1 << 20,
        bottom: -1,
    };
    for y in 0..image.height() {
        for x in 0..image.width() {
            if image.pixel(x, y) == WHITE {
                continue;
            }
            ink.count += 1;
            ink.left = ink.left.min(x as i32);
            ink.right = ink.right.max(x as i32);
            ink.top = ink.top.min(y as i32);
            ink.bottom = ink.bottom.max(y as i32);
        }
    }
    ink
}

/// Whether pixel `c` is ink of `colour`, whole or antialiased onto the white paper: its shortfall
/// from white a share of the colour's own. A 0.25 mm line is one pixel at 4 px a millimetre and
/// usually straddles two, each half covered, so its pixels are the colour faded, never the colour
/// itself.
fn near(c: Color, colour: Color) -> bool {
    let seen = [
        255.0 - f64::from(c.r),
        255.0 - f64::from(c.g),
        255.0 - f64::from(c.b),
    ];
    let full = [
        255.0 - f64::from(colour.r),
        255.0 - f64::from(colour.g),
        255.0 - f64::from(colour.b),
    ];
    let share = seen[0].max(seen[1]).max(seen[2]) / full[0].max(full[1]).max(full[2]);
    if share < 0.25 {
        return false;
    }
    seen.iter()
        .zip(&full)
        .all(|(seen, full)| (seen - share * full).abs() <= 30.0)
}

fn pixels_near(image: &Image, colour: Color) -> usize {
    let mut count = 0;
    for y in 0..image.height() {
        for x in 0..image.width() {
            if near(image.pixel(x, y), colour) {
                count += 1;
            }
        }
    }
    count
}

/// QColor::lightnessF: the mean of the largest and the smallest channel, 0 to 1.
fn lightness(c: Color) -> f64 {
    let high = c.r.max(c.g).max(c.b);
    let low = c.r.min(c.g).min(c.b);
    (f64::from(high) + f64::from(low)) / 2.0 / 255.0
}

/// Separate runs of inked columns along pixel row `y`: the dashes a line shows.
fn runs_along(image: &Image, y: u32) -> i32 {
    let mut runs = 0;
    let mut in_run = false;
    for x in 0..image.width() {
        let mut ink = 0.0;
        for row in y - 2..=y + 2 {
            ink += 1.0 - lightness(image.pixel(x, row));
        }
        let inked = ink >= 0.5;
        if inked && !in_run {
            runs += 1;
        }
        in_run = inked;
    }
    runs
}

fn entry_of(kind: LegendKind, colour: EntityColor) -> LegendEntry {
    LegendEntry {
        kind,
        label: String::from("SAMPLE"),
        colour,
        ..LegendEntry::default()
    }
}

/// How many of the pixels of column 100, rows 40 to 59, are dark.
fn dark_down_the_middle(image: &Image) -> i32 {
    let mut dark = 0;
    for y in 40..60 {
        if lightness(image.pixel(100, y)) < 0.3 {
            dark += 1;
        }
    }
    dark
}

fn solid_fill() -> HatchPattern {
    HatchPattern {
        name: String::from("SOLIDFILL"),
        description: String::new(),
        solid: true,
        families: Vec::new(),
    }
}

// ---- a whole sheet ------------------------------------------------------------------------------

fn plan_at(id: &str, rect: Box2, scale: f64, centre: Point2) -> Viewport {
    Viewport {
        id: String::from(id),
        kind: ViewportKind::Plan,
        rect,
        scale,
        centre,
        ..Viewport::default()
    }
}

fn legend_at(rect: Box2, scope: LegendScope) -> Viewport {
    Viewport {
        id: String::from("vpL"),
        kind: ViewportKind::Legend,
        rect,
        legend_scope: scope,
        ..Viewport::default()
    }
}

fn add_layer(model: &mut Model, name: &str, colour: EntityColor) {
    model
        .layers
        .add(Layer {
            name: String::from(name),
            color: colour,
            ..Layer::default()
        })
        .expect("the layer is added");
}

fn add_entity(model: &mut Model, geometry: Geometry, layer: &str, style: &str) {
    model
        .entities
        .add(Entity {
            geometry,
            layer: String::from(layer),
            style: String::from(style),
            ..Entity::default()
        })
        .expect("the entity is added");
}

fn painted_sheet(set: &SheetSet, model: &Model, name: &str) -> Image {
    let source = SheetSource {
        plan: PlanSource {
            model: Some(model),
            ..PlanSource::default()
        },
        ..SheetSource::default()
    };
    painted(set, &source, name).0
}

fn colour_in(image: &Image, paper: &Box2, colour: Color) -> usize {
    pixels_in(image, paper, |c| near(c, colour))
}

/// The sample cell of row `row` (from 0) in the first column of a legend whose top edge is at
/// `top` (layout_legend's default measures: rows from 8.25 mm under the top, 4.5 mm apart,
/// samples 10 x 3.6 mm from 2.5 mm in).
fn sample_cell(left: f64, top: f64, row: i32) -> Box2 {
    let middle = top - 8.25 - (f64::from(row) + 0.5) * 4.5;
    bx(left + 2.5, middle - 1.8, left + 12.5, middle + 1.8)
}

// ---- the samples --------------------------------------------------------------------------------

mod sheet_legend_sample {
    use super::*;

    #[test]
    fn a_symbol_draws_ink_in_its_cell_at_its_plotted_size() {
        // A built-in circle 1.5 m across at 1:500 prints 3 mm across: 12 pixels, plus its 0.25 mm
        // pen.
        let plot = PlotSettings::default();
        let mut entry = entry_of(LegendKind::Symbol, RED);
        entry.symbol = String::from("circle");
        entry.symbol_size = 1.5;
        let mut image = blank();
        assert!(paint_sample(&mut image, &entry, &context(&plot, None, None)));
        let ink = ink_of(&image);
        assert!(ink.count > 20);
        assert!(
            CELL.adjusted(-1.0, -1.0, 1.0, 1.0)
                .contains_rect(&ink.rect())
        );
        assert!(within(f64::from(ink.width()), 13.0, 2.0), "{}", ink.width());
        assert!(
            within(f64::from(ink.height()), 13.0, 2.0),
            "{}",
            ink.height()
        );
        // Centred in the cell.
        assert!(within(
            0.5 * f64::from(ink.left + ink.right),
            CELL.center().x,
            1.5
        ));
        assert!(within(
            0.5 * f64::from(ink.top + ink.bottom),
            CELL.center().y,
            1.5
        ));
        // In its own colour.
        assert!(pixels_near(&image, Color::rgb(200, 0, 0)) > 10);
    }

    #[test]
    fn a_symbol_too_big_for_its_cell_is_shrunk_to_fit_it() {
        // 10 m at 1:500 would print 20 mm across, in a cell 3.6 mm high.
        let plot = PlotSettings::default();
        let mut entry = entry_of(LegendKind::Symbol, EntityColor::default());
        entry.symbol = String::from("square");
        entry.symbol_size = 10.0;
        let mut image = blank();
        assert!(paint_sample(&mut image, &entry, &context(&plot, None, None)));
        let ink = ink_of(&image);
        assert!(ink.count > 20);
        assert!(ink.height() <= CELL.height as i32 + 2);
        assert!(ink.height() >= CELL.height as i32 - 3);
        assert!(ink.width() <= ink.height() + 2); // still square
    }

    #[test]
    fn a_library_symbol_is_drawn_by_the_shared_style_painter() {
        // A ring of radius 0.5 m on the ground: 2 mm across at 1:500, hollow.
        let plot = PlotSettings::default();
        let ring = LineStyle {
            name: String::from("TEST Ring"),
            at_vertices: true,
            strokes: vec![
                Stroke {
                    op: StrokeOp::Move,
                    point: Point2::new(0.0, 0.0),
                    ..Stroke::default()
                },
                Stroke {
                    op: StrokeOp::Circle,
                    radius: 0.5,
                    ..Stroke::default()
                },
            ],
            ..LineStyle::default()
        };
        let mut library = StyleLibrary::default();
        library.add(ring).expect("the ring is added");
        let mut entry = entry_of(LegendKind::Symbol, EntityColor::default());
        entry.symbol = String::from("TEST Ring");
        let mut image = blank();
        assert!(paint_sample(
            &mut image,
            &entry,
            &context(&plot, None, Some(&library))
        ));
        let ink = ink_of(&image);
        assert!(ink.count > 10);
        assert!(within(f64::from(ink.width()), 9.0, 2.0), "{}", ink.width());
        // Hollow: the middle of the ring is paper.
        assert_eq!(
            image.pixel(
                (0.5 * f64::from(ink.left + ink.right)) as u32,
                (0.5 * f64::from(ink.top + ink.bottom)) as u32
            ),
            WHITE
        );
    }

    #[test]
    fn a_library_linestyle_is_drawn_as_its_own_strokes() {
        // A paper linestyle of a 4 mm period: 1.5 on, 0.5 off, 1 on, 1 off. Along the 10 mm sample
        // that is five dashes; a plain line is one.
        let plot = PlotSettings::default();
        let step = |op: StrokeOp, x: f64| Stroke {
            op,
            point: Point2::new(x, 0.0),
            ..Stroke::default()
        };
        let kerb = LineStyle {
            name: String::from("TEST Dashed Kerb"),
            units: StyleUnits::Paper,
            length: 4.0,
            strokes: vec![
                step(StrokeOp::Move, 0.0),
                step(StrokeOp::Draw, 1.5),
                step(StrokeOp::Move, 2.0),
                step(StrokeOp::Draw, 3.0),
                step(StrokeOp::Move, 4.0),
            ],
            ..LineStyle::default()
        };
        let mut library = StyleLibrary::default();
        library.add(kerb).expect("the kerb is added");
        let mut entry = entry_of(LegendKind::Line, EntityColor::default());
        entry.linetype = String::from("TEST Dashed Kerb");
        let mut dashed = blank();
        assert!(paint_sample(
            &mut dashed,
            &entry,
            &context(&plot, None, Some(&library))
        ));
        assert_eq!(runs_along(&dashed, 50), 5);

        let mut plain = blank();
        entry.linetype = String::from(CONTINUOUS_LINETYPE);
        assert!(paint_sample(
            &mut plain,
            &entry,
            &context(&plot, None, Some(&library))
        ));
        assert_eq!(runs_along(&plain, 50), 1);
        // The cell's whole width.
        let width = ink_of(&plain).width();
        assert!(within(f64::from(width), 40.0, 1.0), "{width}");
    }

    #[test]
    fn a_plain_line_prints_in_its_paper_colour_and_weight() {
        // White prints black on paper; 0.5 mm is two pixels at 4 px a millimetre.
        let plot = PlotSettings::default();
        let mut entry = entry_of(
            LegendKind::Line,
            EntityColor {
                r: 255,
                g: 255,
                b: 255,
                a: 255,
            },
        );
        entry.line_weight = 0.5;
        let mut image = blank();
        assert!(paint_sample(&mut image, &entry, &context(&plot, None, None)));
        assert_eq!(dark_down_the_middle(&image), 2);

        // At a line weight scale of 2, as the plan beside it is drawn: 1 mm.
        let bold = PlotSettings {
            line_weight_scale: 2.0,
            ..PlotSettings::default()
        };
        let mut heavy = blank();
        assert!(paint_sample(&mut heavy, &entry, &context(&bold, None, None)));
        assert_eq!(dark_down_the_middle(&heavy), 4);

        // A red line stays red.
        let mut red = blank();
        assert!(paint_sample(
            &mut red,
            &entry_of(LegendKind::Line, RED),
            &context(&plot, None, None)
        ));
        assert!(pixels_near(&red, Color::rgb(200, 0, 0)) > 30);
    }

    #[test]
    fn an_area_is_a_swatch_filled_with_its_pattern() {
        let plot = PlotSettings::default();
        let mut model = Model::new();
        model
            .hatch_patterns
            .add(solid_fill())
            .expect("the fill is added");
        let mut entry = entry_of(
            LegendKind::Area,
            EntityColor {
                r: 0,
                g: 160,
                b: 0,
                a: 255,
            },
        );
        entry.hatch_pattern = String::from("SOLIDFILL");
        let mut image = blank();
        assert!(paint_sample(
            &mut image,
            &entry,
            &context(&plot, Some(&model), None)
        ));
        let cell = (CELL.width * CELL.height) as usize;
        assert!(pixels_near(&image, Color::rgb(0, 160, 0)) > cell * 6 / 10);
        // Outlined and filled inside the cell, and nowhere else.
        let ink = ink_of(&image);
        assert!(
            CELL.adjusted(-1.0, -1.0, 1.0, 1.0)
                .contains_rect(&ink.rect())
        );
    }

    // In monochrome a solid area prints as the plan prints its fill: black or paper white by its
    // lightness (cad's paper_fill_colour), not in the pen's black whatever its colour - a light
    // swatch beside a dark one would otherwise print the same.
    #[test]
    fn an_areas_fill_follows_the_monochrome_rule_for_fills() {
        let mut model = Model::new();
        model
            .hatch_patterns
            .add(solid_fill())
            .expect("the fill is added");
        let mono = PlotSettings {
            colour_mode: PlotColourMode::Monochrome,
            ..PlotSettings::default()
        };
        let c = context(&mono, Some(&model), None);
        let cell = (CELL.width * CELL.height) as usize;
        let mut light = entry_of(
            LegendKind::Area,
            EntityColor {
                r: 200,
                g: 236,
                b: 200,
                a: 255,
            },
        );
        light.hatch_pattern = String::from("SOLIDFILL");
        let mut pale = blank();
        assert!(paint_sample(&mut pale, &light, &c));
        let mut dark = entry_of(
            LegendKind::Area,
            EntityColor {
                r: 0,
                g: 60,
                b: 0,
                a: 255,
            },
        );
        dark.hatch_pattern = String::from("SOLIDFILL");
        let mut solid = blank();
        assert!(paint_sample(&mut solid, &dark, &c));
        assert!(
            pixels_near(&solid, Color::BLACK) > cell * 6 / 10,
            "a dark fill prints solid"
        );
        // The light one keeps its black outline and nothing more.
        assert!(
            pixels_near(&pale, Color::BLACK) < pixels_near(&solid, Color::BLACK) / 2,
            "a light fill drops out"
        );
    }

    #[test]
    fn a_point_a_text_and_an_empty_cell() {
        let plot = PlotSettings::default();
        let mut point = blank();
        assert!(paint_sample(
            &mut point,
            &entry_of(LegendKind::Point, EntityColor::default()),
            &context(&plot, None, None)
        ));
        // The plain mark: a cross 2 mm (8 px) each way.
        assert!(within(f64::from(ink_of(&point).width()), 9.0, 2.0));
        assert!(within(f64::from(ink_of(&point).height()), 9.0, 2.0));

        let mut text = blank();
        assert!(paint_sample(
            &mut text,
            &entry_of(LegendKind::Text, EntityColor::default()),
            &context(&plot, None, None)
        ));
        assert!(ink_of(&text).count > 20);

        let mut none = blank();
        let mut drawn = true;
        paint_on(&mut none, |canvas| {
            drawn = paint_legend_sample(
                canvas,
                &entry_of(LegendKind::Line, EntityColor::default()),
                RectF::new(10.0, 10.0, 0.0, 12.0),
                &context(&plot, None, None),
            );
        });
        assert!(!drawn);
        assert_eq!(ink_of(&none).count, 0);
    }
}

// ---- the panel on a sheet -----------------------------------------------------------------------

mod sheet_legend_painter {
    use super::*;

    #[test]
    fn the_legend_lists_what_the_plan_beside_it_shows() {
        let mut model = Model::new();
        add_layer(
            &mut model,
            "PITS",
            EntityColor {
                r: 0,
                g: 0,
                b: 0,
                a: 255,
            },
        );
        add_layer(&mut model, "ROAD", RED);
        add_layer(&mut model, "FAR", BLUE);
        model
            .styles
            .add(Style {
                name: String::from("Pit"),
                symbol: String::from("square"),
                ..Style::default()
            })
            .expect("the style is added");
        // The plan is 200 x 200 mm at 1:500 about the origin: 100 m each way.
        add_entity(
            &mut model,
            Geometry::Point(PointGeometry {
                position: Point2::new(0.0, 0.0),
            }),
            "PITS",
            "Pit",
        );
        add_entity(
            &mut model,
            Geometry::Line(Segment2::new(
                Point2::new(-20.0, 5.0),
                Point2::new(20.0, 5.0),
            )),
            "ROAD",
            "",
        );
        add_entity(
            &mut model,
            Geometry::Line(Segment2::new(
                Point2::new(500.0, 500.0),
                Point2::new(510.0, 500.0),
            )),
            "FAR",
            "",
        );
        let legend_rect = bx(300.0, 40.0, 400.0, 145.0);
        let mut set = SheetSet {
            sheets: vec![Sheet {
                id: String::from("s1"),
                viewports: vec![
                    plan_at(
                        "vp1",
                        bx(30.0, 40.0, 230.0, 240.0),
                        500.0,
                        Point2::new(0.0, 0.0),
                    ),
                    legend_at(legend_rect, LegendScope::ThisSheet),
                ],
                ..Sheet::default()
            }],
            ..SheetSet::default()
        };

        // The pit's symbol, then the road; nothing of the far line.
        let own = painted_sheet(&set, &model, "the_legend_lists_sheet");
        assert!(ink_in(&own, &sample_cell(300.0, 145.0, 0)) > 10);
        assert!(colour_in(&own, &sample_cell(300.0, 145.0, 1), Color::rgb(200, 0, 0)) > 10);
        let third = sample_cell(300.0, 145.0, 2);
        assert_eq!(
            ink_in(&own, &bx(301.0, third.min.y, 399.0, third.max.y)),
            0
        );

        // The whole drawing lists the far line too, between them: FAR, then ROAD.
        set.sheets[0].viewports[1].legend_scope = LegendScope::WholeDrawing;
        let drawing = painted_sheet(&set, &model, "the_legend_lists_drawing");
        assert!(ink_in(&drawing, &sample_cell(300.0, 145.0, 0)) > 10);
        assert!(
            colour_in(
                &drawing,
                &sample_cell(300.0, 145.0, 1),
                Color::rgb(0, 0, 255)
            ) > 10
        );
        assert!(
            colour_in(
                &drawing,
                &sample_cell(300.0, 145.0, 2),
                Color::rgb(200, 0, 0)
            ) > 10
        );
    }

    #[test]
    fn what_does_not_fit_is_counted_in_the_last_place() {
        // A panel 40 x 60 mm holds one column of ten rows. Thirty blue layers leave nine samples
        // and "+21 more" in the tenth place, in grey; ten layers fill all ten places with samples.
        let sheet_with = |layers: i32, model: &mut Model| {
            for i in 1..=layers {
                let name = format!("LAYER {i:02}");
                add_layer(model, &name, BLUE);
                add_entity(
                    model,
                    Geometry::Line(Segment2::new(
                        Point2::new(-10.0, f64::from(i)),
                        Point2::new(10.0, f64::from(i)),
                    )),
                    &name,
                    "",
                );
            }
            SheetSet {
                sheets: vec![Sheet {
                    id: String::from("s1"),
                    viewports: vec![
                        plan_at(
                            "vp1",
                            bx(30.0, 40.0, 230.0, 240.0),
                            500.0,
                            Point2::new(0.0, 0.0),
                        ),
                        legend_at(bx(300.0, 40.0, 340.0, 100.0), LegendScope::ThisSheet),
                    ],
                    ..Sheet::default()
                }],
                ..SheetSet::default()
            }
        };
        let blue = Color::rgb(0, 0, 255);
        let mut many = Model::new();
        let set = sheet_with(30, &mut many);
        let overflowing = painted_sheet(&set, &many, "what_does_not_fit_30");
        for row in 0..9 {
            assert!(
                colour_in(&overflowing, &sample_cell(300.0, 100.0, row), blue) > 10,
                "row {row}"
            );
        }
        let last = sample_cell(300.0, 100.0, 9);
        assert!(ink_in(&overflowing, &last) > 10);
        assert_eq!(colour_in(&overflowing, &last, blue), 0);

        let mut ten = Model::new();
        let set = sheet_with(10, &mut ten);
        let fitting = painted_sheet(&set, &ten, "what_does_not_fit_10");
        assert!(colour_in(&fitting, &last, blue) > 10);
    }
}
