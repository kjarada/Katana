//! The sheet editor's frames, measured (CLAUDE.md section 5; the practice of docs/testing.md,
//! "Benchmarks"; docs/performance.md, "The sheet editor's canvas"): what a frame costs an editor
//! over a sheet when nothing on its canvas changed - the frame egui paints for every pointer move
//! anywhere in the window - and what it costs when the canvas's own marks change.
//!
//! The drawing is generated here: 20 000 lines of 2 to 8 units scattered over a site of 400 by
//! 250 units, on one A3 sheet of two plans (one drawn at the scale that fits the drawing, one at
//! 1:250) and a note. The frames are the whole editor's - its menus, toolbar, sheet list,
//! properties and checks beside the canvas - through egui, headless, on a 1400 x 900 screen. Only
//! public members that the base build has too are used, so this source measures the base build
//! and the changed one alike. Cells, alternated round by round with the order reversed every
//! round, each round taking `REPEATS` samples of each:
//!
//! * `idle_frames` and `idle_frames_again`: `IDLE_FRAMES` frames with nothing changed between
//!   them, under two names, the A/A control;
//! * `select_frames`: `CHANGING_FRAMES` frames, each with another of the sheet's three viewports
//!   picked first - the outline and handles move, the painted sheet under them is the kept one;
//! * `pointer_frames`: `CHANGING_FRAMES` frames, each with the pointer moved a few pixels across
//!   the paper first - the rulers' cursor marks move.
//!
//! Prints one `cell=... min_ms=... median_ms=... samples=... uploads=...` line a cell, the last
//! being how many frames of the cell's last sample handed the canvas's picture to its texture.
//! Headless, a texture handed over is only queued: on a desktop it is then uploaded to the
//! graphics card, which this does not time. Never the owner's data. Run in Release:
//! `cargo bench --locked -p katana_window --bench sheet_frames` (or through
//! `cargo test --release`, which runs it too, one round); `KATANA_BENCH_ROUNDS` sets the rounds
//! (default 7).

use std::rc::Rc;
use std::time::{Duration, Instant};

use katana_cad::Document;
use katana_cad::plotting::sheet_set::{Sheet, SheetSet, Viewport, ViewportKind};
use katana_commands::create_entities;
use katana_entity::{Entity, Geometry};
use katana_geometry::{Box2, Point2, Segment2};
use katana_window::drive::Harness;
use katana_window::host::{Answer, RecordingHost};
use katana_window::plan_painter::plan_source_of;
use katana_window::sheet_editor::{SheetEditor, SourceProvider};
use katana_window::sheet_painter::SheetSource;

const SCREEN_WIDTH: f32 = 1400.0;
const SCREEN_HEIGHT: f32 = 900.0;
const LINES: usize = 20_000;
const SITE_WIDTH: f64 = 400.0;
const SITE_HEIGHT: f64 = 250.0;
const IDLE_FRAMES: usize = 30;
const CHANGING_FRAMES: usize = 10;
const REPEATS: usize = 3;
/// The sheet's viewports, in the order `select_frames` picks them.
const VIEWPORTS: [&str; 3] = ["vp1", "vp2", "vp3"];
/// How many places the pointer visits on the paper before it starts again.
const POINTER_PLACES: usize = 40;

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

/// A rectangle of the paper, in millimetres from its bottom-left corner.
fn paper(x0: f64, y0: f64, x1: f64, y1: f64) -> Box2 {
    Box2::new(Point2::new(x0, y0), Point2::new(x1, y1))
}

/// One A3 sheet, landscape (420 x 297 mm): two plans side by side over a note.
fn sheets() -> SheetSet {
    let middle = Point2::new(SITE_WIDTH / 2.0, SITE_HEIGHT / 2.0);
    SheetSet {
        sheets: vec![Sheet {
            id: String::from("s1"),
            name: String::from("PLAN"),
            viewports: vec![
                Viewport {
                    id: String::from(VIEWPORTS[0]),
                    kind: ViewportKind::Plan,
                    rect: paper(30.0, 120.0, 200.0, 270.0),
                    auto_scale: true,
                    auto_centre: true,
                    ..Viewport::default()
                },
                Viewport {
                    id: String::from(VIEWPORTS[1]),
                    kind: ViewportKind::Plan,
                    rect: paper(210.0, 120.0, 380.0, 270.0),
                    scale: 250.0,
                    centre: middle,
                    ..Viewport::default()
                },
                Viewport {
                    id: String::from(VIEWPORTS[2]),
                    kind: ViewportKind::Notes,
                    rect: paper(30.0, 40.0, 200.0, 110.0),
                    text: String::from("Generated lines, for the benchmark."),
                    ..Viewport::default()
                },
            ],
            ..Sheet::default()
        }],
        ..SheetSet::default()
    }
}

/// What the sheets are drawn from: the drawing's plan and its revision, as the editor's tests
/// give theirs.
fn source() -> SourceProvider {
    Rc::new(|document: &Document| SheetSource {
        plan: plan_source_of(document),
        revision: document.model_revision(),
        ..SheetSource::default()
    })
}

/// The editor over the drawing, filling a headless egui window.
struct Window {
    harness: Harness,
    host: RecordingHost,
    editor: SheetEditor,
    /// Where the canvas is in the window, and its picture's size in pixels.
    place: egui::Rect,
    canvas: [usize; 2],
    selected: usize,
    pointer: usize,
}

impl Window {
    fn new(lines: Vec<Entity>) -> Self {
        let mut document = Document::new(None);
        document
            .execute(create_entities(lines))
            .expect("the lines are made");
        document
            .set_sheet_set(&sheets(), Document::SET_SHEETS)
            .expect("the sheets are set");
        let mut window = Self {
            harness: Harness::with_screen(SCREEN_WIDTH, SCREEN_HEIGHT),
            host: RecordingHost::with_document(document).answering(Answer::Interpreter),
            editor: SheetEditor::new(source()),
            place: egui::Rect::NOTHING,
            canvas: [0, 0],
            selected: 0,
            pointer: 0,
        };
        window.editor.show(&window.host);
        // What the first frames set going is let run before anything is timed: the sheet is
        // painted, and a moment later the list's thumbnail and the checks (their own clocks,
        // 30 ms and 400 ms), whose findings lay the Checks dock out again under the canvas.
        for _ in 0..12 {
            let _ = window.frame();
            std::thread::sleep(Duration::from_millis(50));
        }
        for _ in 0..4 {
            let _ = window.frame();
        }
        window.place = window
            .harness
            .control("sheetCanvas")
            .expect("the canvas is drawn")
            .rect;
        let canvas = window.editor.canvas();
        window.canvas = [canvas.width() as usize, canvas.height() as usize];
        window
    }

    /// One frame; whether the canvas's picture was handed to its texture in it.
    fn frame(&mut self) -> bool {
        let (editor, host) = (&mut self.editor, &mut self.host);
        let output = self
            .harness
            .frame(|ui: &mut egui::Ui| editor.ui(ui, &mut *host));
        // The canvas's texture is the only one of the canvas's size.
        let size = self.canvas;
        output
            .textures_delta
            .set
            .values()
            .flatten()
            .any(|delta| delta.pos.is_none() && delta.image.size() == size)
    }

    /// `count` frames with nothing changed: their milliseconds, and how many handed the
    /// canvas's picture to the texture.
    fn idle_frames(&mut self, count: usize) -> (f64, usize) {
        let mut uploads = 0;
        let started = Instant::now();
        for _ in 0..count {
            uploads += usize::from(self.frame());
        }
        (started.elapsed().as_secs_f64() * 1.0e3, uploads)
    }

    /// `count` frames, each with the next viewport picked first.
    fn select_frames(&mut self, count: usize) -> (f64, usize) {
        let mut uploads = 0;
        let started = Instant::now();
        for _ in 0..count {
            self.selected = (self.selected + 1) % VIEWPORTS.len();
            self.editor.canvas_mut().select(VIEWPORTS[self.selected]);
            uploads += usize::from(self.frame());
        }
        (started.elapsed().as_secs_f64() * 1.0e3, uploads)
    }

    /// `count` frames, each with the pointer moved first: three pixels across and two down from
    /// the last place, about the canvas's middle, where the paper is.
    fn pointer_frames(&mut self, count: usize) -> (f64, usize) {
        let mut uploads = 0;
        let started = Instant::now();
        for _ in 0..count {
            self.pointer = (self.pointer + 1) % POINTER_PLACES;
            let step = self.pointer as f32 - POINTER_PLACES as f32 / 2.0;
            let at = self.place.center() + egui::vec2(3.0 * step, 2.0 * step);
            self.harness.pointer_move(at);
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
    let mut window = Window::new(lines());

    let mut cells = [
        "idle_frames",
        "select_frames",
        "pointer_frames",
        "idle_frames_again",
    ];
    // A cell's samples, and the uploads its last sample counted.
    let mut samples: Vec<(&str, Vec<f64>, usize)> =
        cells.iter().map(|cell| (*cell, Vec::new(), 0)).collect();
    for round in 0..rounds {
        if round % 2 == 1 {
            cells.reverse();
        }
        for cell in cells {
            for _ in 0..REPEATS {
                let (taken, uploads) = match cell {
                    "select_frames" => window.select_frames(CHANGING_FRAMES),
                    "pointer_frames" => window.pointer_frames(CHANGING_FRAMES),
                    _ => window.idle_frames(IDLE_FRAMES),
                };
                if let Some((_, kept, last)) = samples.iter_mut().find(|(name, _, _)| *name == cell)
                {
                    kept.push(taken);
                    *last = uploads;
                }
            }
        }
        if round % 2 == 1 {
            cells.reverse();
        }
    }
    println!(
        "lines={LINES} screen={SCREEN_WIDTH}x{SCREEN_HEIGHT} canvas={}x{} \
         idle_frames={IDLE_FRAMES} changing_frames={CHANGING_FRAMES}",
        window.canvas[0], window.canvas[1]
    );
    for (cell, mut kept, uploads) in samples {
        kept.sort_by(f64::total_cmp);
        let median = kept[kept.len() / 2];
        println!(
            "cell={cell} min_ms={:.3} median_ms={:.3} samples={} uploads={uploads}",
            kept[0],
            median,
            kept.len()
        );
    }
}
