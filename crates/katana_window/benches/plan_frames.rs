//! The plan view's frames, measured (CLAUDE.md section 5; the practice of docs/testing.md,
//! "Benchmarks"; docs/performance.md, "The plan view's frames" and "The labels' walk"): what a
//! frame costs a plan view over a large drawing when nothing in it changed - the frame egui
//! paints for every pointer move anywhere in the window - what a pan costs it, and what a
//! zoomed-in repaint pays for a drawing that has a label style.
//!
//! The drawing is generated here: 100 000 lines of 2 to 8 units scattered over a site of 2 000
//! by 1 250 units, as the strings of a large survey lie. Only public members that the base build
//! has too are used, so this source measures the base build and the changed one alike. Cells,
//! alternated round by round with the order reversed every round, each round taking `REPEATS`
//! samples of each:
//!
//! * `idle_frames` and `idle_frames_again`: `IDLE_FRAMES` frames of a 1600 x 1000 plan view
//!   through egui, headless, with nothing changed between them, under two names, the A/A
//!   control;
//! * `pan_frames`: `PAN_FRAMES` frames of the same view, zoomed in ten times, each panned three
//!   pixels - every frame repaints the drawing and changes the picture;
//! * `paint_zoomed`: `paint_plan` of a hundredth of the drawing's area, the drawing with no
//!   label style;
//! * `paint_zoomed_label_style`: the same paint of the same lines in a drawing that has one
//!   label style (and no label), which is what makes the painter look for labels;
//! * `idle_frames_selected`: `IDLE_FRAMES` frames with nothing changed of a second view of the
//!   same lines with every one of them selected - the selection is part of what the kept drawing
//!   is keyed by, and every frame looks at it (docs/performance.md, "The selection in the plan
//!   view's key").
//!
//! Prints one `cell=... min_ms=... median_ms=... samples=...` line a cell; the frame cells'
//! lines end with how many times the view's picture was handed to its texture in the last
//! sample (`uploads=`), which is where the two builds are meant to differ. Headless, a texture
//! handed over is only queued: on a desktop it is then uploaded to the graphics card, which
//! this does not time. Never the owner's data. Run in Release:
//! `cargo bench --locked -p katana_window --bench plan_frames` (or through
//! `cargo test --release`, which runs it too, one round); `KATANA_BENCH_ROUNDS` sets the rounds
//! (default 7).

use std::hint::black_box;
use std::time::Instant;

use egui::UiBuilder;
use katana_cad::Document;
use katana_cad::spatial_query::query_extents;
use katana_cad::view_set::{ViewSet, ViewState};
use katana_cad::viewport_layout::ViewKind;
use katana_commands::create_entities;
use katana_entity::annotation::LabelStyle;
use katana_entity::{Entity, Geometry, Model};
use katana_geometry::{Box2, Point2, Segment2, SpatialEntry, SpatialIndex};
use katana_window::canvas::{Brush, Canvas, Color, Image, RasterCanvas, RectF};
use katana_window::drive::Harness;
use katana_window::plan_painter::{
    PlanFrame, PlanPaintCache, PlanPaintOptions, PlanSource, paint_plan,
};
use katana_window::viewport_widget::PlanView;

const WIDTH: usize = 1600;
const HEIGHT: usize = 1000;
const LINES: usize = 100_000;
const SITE_WIDTH: f64 = 2_000.0;
const SITE_HEIGHT: f64 = 1_250.0;
const IDLE_FRAMES: usize = 30;
const PAN_FRAMES: usize = 10;
const REPEATS: usize = 3;

/// Knuth's MMIX linear congruential generator (The Art of Computer Programming, volume 2,
/// section 3.3.4, table 1, line 26), as a real in [0, 1): the same lines on every machine.
struct Sequence(u64);

impl Sequence {
    fn real(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 11) as f64 * (1.0 / 9_007_199_254_740_992.0)
    }
}

fn lines() -> Vec<Entity> {
    let mut sequence = Sequence(2026);
    (0..LINES)
        .map(|_| {
            let start = Point2::new(sequence.real() * SITE_WIDTH, sequence.real() * SITE_HEIGHT);
            let heading = std::f64::consts::TAU * sequence.real();
            let length = 2.0 + 6.0 * sequence.real();
            let end = Point2::new(
                start.x + length * heading.cos(),
                start.y + length * heading.sin(),
            );
            Entity {
                geometry: Geometry::Line(Segment2::new(start, end)),
                ..Entity::default()
            }
        })
        .collect()
}

/// The lines as a model with its index, as a document keeps them.
fn model_of(lines: &[Entity], label_style: bool) -> (Model, SpatialIndex) {
    let mut model = Model::default();
    for line in lines {
        model.entities.add(line.clone()).expect("a line is added");
    }
    if label_style {
        model
            .label_styles
            .add(LabelStyle {
                name: String::from("spot"),
                ..LabelStyle::default()
            })
            .expect("a label style is added");
    }
    let mut entries = Vec::with_capacity(lines.len());
    model.entities.for_each(|entity| {
        entries.push(SpatialEntry {
            id: entity.id,
            box_value: query_extents(&model, entity),
        });
    });
    let mut index = SpatialIndex::default();
    index.rebuild(&entries);
    (model, index)
}

/// A tenth of the site each way about its middle, nudged by `nudge` pixels.
fn zoomed_frame(nudge: f64) -> PlanFrame<'static> {
    let site = Box2::new(
        Point2::new(0.0, 0.0),
        Point2::new(SITE_WIDTH, SITE_HEIGHT),
    );
    let mut frame = PlanFrame::default();
    frame.transform.resize(WIDTH as f64, HEIGHT as f64);
    frame.transform.fit(&site, 0.02);
    frame.transform.scale *= 10.0;
    frame.transform.center.x += nudge / frame.transform.scale;
    frame
}

/// One `paint_plan` of the zoomed view, in milliseconds, and how many entities it drew.
fn paint(
    model: &Model,
    index: &SpatialIndex,
    nudge: f64,
    cache: &mut PlanPaintCache,
    image: &mut Image,
) -> (f64, usize) {
    let source = PlanSource {
        model: Some(model),
        index: Some(index),
        ..PlanSource::default()
    };
    let frame = zoomed_frame(nudge);
    let options = PlanPaintOptions::default();
    let mut drawn = 0;
    let started = Instant::now();
    RasterCanvas::paint_image(image, &mut |canvas: &mut dyn Canvas| {
        canvas.fill_rect(
            RectF::new(0.0, 0.0, WIDTH as f64, HEIGHT as f64),
            &Brush::solid(Color::rgb(0x1e, 0x23, 0x29)),
        );
        drawn = paint_plan(canvas, &source, &frame, &options, cache).entities_drawn;
    });
    (started.elapsed().as_secs_f64() * 1.0e3, drawn)
}

/// A plan view of the drawing filling a headless egui window.
struct Window {
    harness: Harness,
    document: Document,
    state: ViewState,
    view: PlanView,
}

impl Window {
    fn new(lines: Vec<Entity>) -> Self {
        let mut document = Document::new(None);
        document
            .execute(create_entities(lines))
            .expect("the lines are made");
        let mut views = ViewSet::new();
        let mut state = views.add(ViewKind::Plan).clone();
        let view = PlanView::new(&document, &mut state);
        let mut window = Self {
            harness: Harness::with_screen(WIDTH as f32, HEIGHT as f32),
            document,
            state,
            view,
        };
        // The first frame frames the drawing and paints it; then ten times closer, where a
        // repaint draws a hundredth of the lines.
        let _ = window.frame();
        window.state.plan.scale *= 10.0;
        let _ = window.frame();
        window
    }

    /// Every line selected, announced as a person's selection is, and the frame that draws it.
    fn select_all(&mut self) {
        let mut ids = Vec::with_capacity(LINES);
        self.document
            .model()
            .entities
            .for_each(|entity| ids.push(entity.id));
        self.document.selection_mut().set(ids);
        self.document.notify_selection_changed();
        let _ = self.frame();
    }

    /// One frame; whether the view's picture was handed to its texture in it.
    fn frame(&mut self) -> bool {
        let place = self.harness.screen();
        let (view, document, state) = (&mut self.view, &mut self.document, &mut self.state);
        let output = self.harness.frame(|ui: &mut egui::Ui| {
            ui.scope_builder(UiBuilder::new().max_rect(place), |ui| {
                let _ = view.ui(ui, &mut *document, &mut *state);
            });
        });
        // The view's texture is the only one of the window's size.
        output
            .textures_delta
            .set
            .values()
            .flatten()
            .any(|delta| delta.pos.is_none() && delta.image.size() == [WIDTH, HEIGHT])
    }

    /// `count` frames, each panned `pan` pixels first: their milliseconds, and how many handed
    /// the picture to the texture.
    fn frames(&mut self, count: usize, pan: f64) -> (f64, usize) {
        let mut uploads = 0;
        let started = Instant::now();
        for _ in 0..count {
            if pan != 0.0 {
                self.state.plan.center.x += pan / self.state.plan.scale;
            }
            uploads += usize::from(self.frame());
        }
        (started.elapsed().as_secs_f64() * 1.0e3, uploads)
    }
}

fn main() {
    let rounds: usize = std::env::var("KATANA_BENCH_ROUNDS")
        .ok()
        .and_then(|text| text.parse().ok())
        .unwrap_or(7);
    // `cargo test --release` runs a bench target as a test with `--bench` absent: once, quickly.
    let quick = !std::env::args().any(|argument| argument == "--bench");
    let rounds = if quick { 1 } else { rounds.max(1) };
    let lines = lines();
    let (plain, plain_index) = model_of(&lines, false);
    let (labelled, labelled_index) = model_of(&lines, true);
    let mut selected = Window::new(lines.clone());
    selected.select_all();
    let mut window = Window::new(lines);
    let mut cache = PlanPaintCache::default();
    let mut image = Image::new(WIDTH as u32, HEIGHT as u32);

    let mut cells = [
        "idle_frames",
        "pan_frames",
        "paint_zoomed",
        "paint_zoomed_label_style",
        "idle_frames_again",
        "idle_frames_selected",
    ];
    // A cell's samples, and the count its last sample gave: uploads, or entities drawn.
    let mut samples: Vec<(&str, Vec<f64>, usize)> =
        cells.iter().map(|cell| (*cell, Vec::new(), 0)).collect();
    let mut nudge = 0.0;
    for round in 0..rounds {
        if round % 2 == 1 {
            cells.reverse();
        }
        for cell in cells {
            for _ in 0..REPEATS {
                // A third of a pixel between paints, so no paint is the one before it again.
                nudge = if nudge == 0.0 { 1.0 / 3.0 } else { 0.0 };
                let (taken, count) = match cell {
                    "pan_frames" => window.frames(PAN_FRAMES, 3.0),
                    "paint_zoomed" => paint(&plain, &plain_index, nudge, &mut cache, &mut image),
                    "paint_zoomed_label_style" => {
                        paint(&labelled, &labelled_index, nudge, &mut cache, &mut image)
                    }
                    "idle_frames_selected" => selected.frames(IDLE_FRAMES, 0.0),
                    _ => window.frames(IDLE_FRAMES, 0.0),
                };
                if let Some((_, kept, last)) = samples.iter_mut().find(|(name, _, _)| *name == cell)
                {
                    kept.push(taken);
                    *last = count;
                }
            }
        }
        if round % 2 == 1 {
            cells.reverse();
        }
    }
    black_box(image.bytes().len());
    println!(
        "lines={LINES} view={WIDTH}x{HEIGHT} idle_frames={IDLE_FRAMES} pan_frames={PAN_FRAMES}"
    );
    for (cell, mut kept, count) in samples {
        kept.sort_by(f64::total_cmp);
        let median = kept[kept.len() / 2];
        let counted = if cell.contains("frames") {
            "uploads"
        } else {
            "entities_drawn"
        };
        println!(
            "cell={cell} min_ms={:.3} median_ms={:.3} samples={} {counted}={count}",
            kept[0],
            median,
            kept.len()
        );
    }
}
